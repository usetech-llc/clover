//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.

use std::{
  sync::{Arc,},
};

use cumulus_client_consensus_relay_chain::{
	build_relay_chain_consensus, BuildRelayChainConsensusParams,
};
use cumulus_client_network::build_block_announce_validator;
use cumulus_client_service::{
  prepare_node_config, start_collator, start_full_node, StartCollatorParams, StartFullNodeParams,
};

use polkadot_primitives::v0::CollatorPair;

use clover_runtime::{self, opaque::Block, RuntimeApi};
use sp_core::Pair;
use sc_service::{error::Error as ServiceError, Configuration, Role, TaskManager, TFullClient,};
use sc_telemetry::{Telemetry, TelemetryWorker, TelemetryWorkerHandle};
use sp_runtime::traits::BlakeTwo256;
use sp_trie::PrefixedMemoryDB;
use sc_executor::native_executor_instance;
pub use sc_executor::NativeExecutor;
use fc_consensus::FrontierBlockImport;

// Our native executor instance.
native_executor_instance!(
  pub Executor,
  clover_runtime::api::dispatch,
  clover_runtime::native_version,
);

/// Build the inherent data providers timestamp for the node.
pub fn build_inherent_data_providers() -> Result<sp_inherents::InherentDataProviders, sc_service::Error> {
  let providers = sp_inherents::InherentDataProviders::new();

  providers
    .register_provider(sp_timestamp::InherentDataProvider)
    .map_err(Into::into)
    .map_err(sp_consensus::error::Error::InherentData)?;

  Ok(providers)
}

type FullClient = sc_service::TFullClient<Block, RuntimeApi, Executor>;
type FullBackend = sc_service::TFullBackend<Block>;

pub fn new_partial(config: &Configuration) -> Result<sc_service::PartialComponents<
    FullClient,
    FullBackend,
    (),
    sp_consensus::import_queue::BasicQueue<Block, PrefixedMemoryDB<BlakeTwo256>>,
    sc_transaction_pool::FullPool<Block, FullClient>,
    (FrontierBlockImport<Block, Arc<FullClient>, FullClient>, Option<Telemetry>, Option<TelemetryWorkerHandle>),
>, ServiceError> {
  let inherent_data_providers = build_inherent_data_providers()?;

  let telemetry = config.telemetry_endpoints.clone()
		.filter(|x| !x.is_empty())
		.map(|endpoints| -> Result<_, sc_telemetry::Error> {
			let worker = TelemetryWorker::new(16)?;
			let telemetry = worker.handle().new_telemetry(endpoints);
			Ok((worker, telemetry))
		})
		.transpose()?;

  let (client, backend, keystore_container, task_manager) =
    sc_service::new_full_parts::<Block, RuntimeApi, Executor>(&config,
      telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()))?;
  let client = Arc::new(client);

  let telemetry_worker_handle = telemetry
    .as_ref()
    .map(|(worker, _)| worker.handle());

  let telemetry = telemetry
    .map(|(worker, telemetry)| {
      task_manager.spawn_handle().spawn("telemetry", worker.run());
      telemetry
    });

  let registry = config.prometheus_registry();

  let transaction_pool = sc_transaction_pool::BasicPool::new_full(
    config.transaction_pool.clone(),
    config.role.is_authority().into(),
    config.prometheus_registry(),
    task_manager.spawn_handle(),
    client.clone(),
  );

  let frontier_block_import = FrontierBlockImport::new(client.clone(), client.clone(), true);

  let import_queue = cumulus_client_consensus_relay_chain::import_queue(
    client.clone(),
    frontier_block_import.clone(),
    inherent_data_providers.clone(),
    &task_manager.spawn_essential_handle(),
    registry.clone(),
  )?;


  Ok(sc_service::PartialComponents {
    client, backend, task_manager, keystore_container,
    select_chain: (),
    import_queue, transaction_pool,
    inherent_data_providers,
    other: (frontier_block_import, telemetry, telemetry_worker_handle),
  })
}

/// Start a node with the given parachain `Configuration` and relay chain `Configuration`.
///
/// This is the actual implementation that is abstract over the executor and the runtime api.
async fn start_node_impl<RB>(
  parachain_config: Configuration,
  collator_key: CollatorPair,
  polkadot_config: Configuration,
  id: polkadot_primitives::v0::Id,
  validator: bool,
  _rpc_ext_builder: RB,
) -> sc_service::error::Result<(TaskManager, Arc<FullClient>)>
where
  RB: Fn(
      Arc<TFullClient<Block, RuntimeApi, Executor>>,
    ) -> jsonrpc_core::IoHandler<sc_rpc::Metadata>
    + Send
    + 'static,
{
  if matches!(parachain_config.role, Role::Light) {
    return Err("Light client not supported!".into());
  }

  let parachain_config = prepare_node_config(parachain_config);

  let params = new_partial(&parachain_config)?;

  let (block_import, mut telemetry, telemetry_worker_handle) = params.other;

  let polkadot_full_node =
    cumulus_client_service::build_polkadot_full_node(
      polkadot_config,
      collator_key.clone(),
      telemetry_worker_handle).map_err(
      |e| match e {
        polkadot_service::Error::Sub(x) => x,
        s => format!("{}", s).into(),
      },
    )?;

  let client = params.client.clone();
  let backend = params.backend.clone();
  let block_announce_validator = build_block_announce_validator(
    polkadot_full_node.client.clone(),
    id,
    Box::new(polkadot_full_node.network.clone()),
    polkadot_full_node.backend.clone(),
  );

  let prometheus_registry = parachain_config.prometheus_registry().cloned();
  let transaction_pool = params.transaction_pool.clone();
  let mut task_manager = params.task_manager;
  let import_queue = params.import_queue;

  let (network, network_status_sinks, system_rpc_tx, start_network) =
    sc_service::build_network(sc_service::BuildNetworkParams {
      config: &parachain_config,
      client: client.clone(),
      transaction_pool: transaction_pool.clone(),
      spawn_handle: task_manager.spawn_handle(),
      import_queue,
      on_demand: None,
      block_announce_validator_builder: Some(Box::new(|_| block_announce_validator)),
    })?;

  let is_authority = parachain_config.role.is_authority();
  let subscription_task_executor =
    sc_rpc::SubscriptionTaskExecutor::new(task_manager.spawn_handle());

  let rpc_extensions_builder = {
    let client = client.clone();
    let pool = transaction_pool.clone();
    let network = network.clone();
    // let pending = pending_transactions.clone();
    Box::new(move |deny_unsafe, _| {
      let deps = crate::rpc::FullDeps {
        client: client.clone(),
        pool: pool.clone(),
        graph: pool.pool().clone(),
        deny_unsafe,
        is_authority,
        network: network.clone(),
        //pending_transactions: pending.clone(),
        //command_sink: None,
      };

      crate::rpc::create_full(deps, subscription_task_executor.clone())
    })
  };

  sc_service::spawn_tasks(sc_service::SpawnTasksParams {
    on_demand: None,
    remote_blockchain: None,
    rpc_extensions_builder,
    client: client.clone(),
    transaction_pool: transaction_pool.clone(),
    task_manager: &mut task_manager,
    config: parachain_config,
    keystore: params.keystore_container.sync_keystore(),
    backend: backend.clone(),
    network: network.clone(),
    network_status_sinks,
    system_rpc_tx,
    telemetry: telemetry.as_mut(),
  })?;

  let announce_block = {
    let network = network.clone();
    Arc::new(move |hash, data| network.announce_block(hash, data))
  };

  if validator {
    let proposer_factory = sc_basic_authorship::ProposerFactory::with_proof_recording(
			task_manager.spawn_handle(),
			client.clone(),
			transaction_pool,
			prometheus_registry.as_ref(),
      telemetry.as_ref().map(|x| x.handle()),
		);

    let spawner = task_manager.spawn_handle();

    let parachain_consensus = build_relay_chain_consensus(BuildRelayChainConsensusParams {
			para_id: id,
			proposer_factory,
			inherent_data_providers: params.inherent_data_providers,
			block_import,
			relay_chain_client: polkadot_full_node.client.clone(),
			relay_chain_backend: polkadot_full_node.backend.clone(),
		});

    //let polkadot_backend = polkadot_full_node.backend.clone();

    let params = StartCollatorParams {
      para_id: id,
      block_status: client.clone(),
      announce_block,
      client: client.clone(),
      task_manager: &mut task_manager,
      collator_key,
      relay_chain_full_node: polkadot_full_node,
      spawner,
      backend,
      parachain_consensus,
    };

    start_collator(params).await?;
  } else {
    let params = StartFullNodeParams {
      client: client.clone(),
      announce_block,
      task_manager: &mut task_manager,
      para_id: id,
      polkadot_full_node,
    };

    start_full_node(params)?;
  }

  start_network.start_network();

  Ok((task_manager, client))
}

/// Start a normal parachain node.
pub async fn start_node(
  parachain_config: Configuration,
  collator_key: CollatorPair,
  polkadot_config: Configuration,
  id: polkadot_primitives::v0::Id,
  validator: bool,
) -> sc_service::error::Result<(TaskManager, Arc<FullClient>)> {
  start_node_impl(
    parachain_config,
    collator_key,
    polkadot_config,
    id,
    validator,
    |_| Default::default(),
  )
  .await
}
