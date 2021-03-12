use cumulus_primitives_core::ParaId;
use sc_chain_spec::{ChainSpecExtension, ChainSpecGroup};
use serde_json::json;
use serde::{Deserialize, Serialize};
use sp_core::{Pair, Public, sr25519, U256};
use clover_runtime::{
  AccountId, Balance, BalancesConfig, ContractsConfig, CurrencyId, IndicesConfig, GenesisConfig, /*ImOnlineId,*/
   SudoConfig, SystemConfig, WASM_BINARY,
  Signature, TokensConfig, IncentivesConfig, CloverDexConfig, BandOracleConfig,
  CloverOracleConfig, EVMConfig, EthereumConfig, DOLLARS
};
use sp_runtime::{traits::{IdentifyAccount, Verify},};
use sc_service::ChainType;
use hex_literal::hex;
use sc_telemetry::TelemetryEndpoints;
use std::collections::BTreeMap;
use clover_evm::GenesisAccount;
use primitive_types::H160;
use std::str::FromStr;

// The URL for the telemetry server.
const TELEMETRY_URL: &str = "wss://telemetry.polkadot.io/submit/";

/// Specialized `ChainSpec`. This is a specialization of the general Substrate ChainSpec type.
pub type ChainSpec = sc_service::GenericChainSpec<clover_runtime::GenesisConfig, Extensions>;

// fn session_keys(
// //  grandpa: GrandpaId,
// //  babe: BabeId,
// //  im_online: ImOnlineId,
// ) -> SessionKeys {
//   SessionKeys {} // grandpa, babe, im_online, }
// }

/// Generate a crypto pair from seed.
pub fn get_from_seed<TPublic: Public>(seed: &str) -> <TPublic::Pair as Pair>::Public {
  TPublic::Pair::from_string(&format!("//{}", seed), None)
    .expect("static values are valid; qed")
    .public()
}


/// The extensions for the [`ChainSpec`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ChainSpecGroup, ChainSpecExtension)]
#[serde(deny_unknown_fields)]
pub struct Extensions {
	/// The relay chain of the Parachain.
	pub relay_chain: String,
	/// The id of the Parachain.
	pub para_id: u32,
}

impl Extensions {
	/// Try to get the extension from the given `ChainSpec`.
	pub fn try_get(chain_spec: &dyn sc_service::ChainSpec) -> Option<&Self> {
		sc_chain_spec::get_extension(chain_spec.extensions())
	}
}

type AccountPublic = <Signature as Verify>::Signer;

/// Generate an account ID from seed.
pub fn get_account_id_from_seed<TPublic: Public>(seed: &str) -> AccountId where
  AccountPublic: From<<TPublic::Pair as Pair>::Public>
{
  AccountPublic::from(get_from_seed::<TPublic>(seed)).into_account()
}

fn endowed_evm_account() -> BTreeMap<H160, GenesisAccount>{
  let endowed_account = vec![
    H160::from_str("6be02d1d3665660d22ff9624b7be0551ee1ac91b").unwrap(),
    H160::from_str("e6206C7f064c7d77C6d8e3eD8601c9AA435419cE").unwrap()
  ];
  get_endowed_evm_accounts(endowed_account)
}

fn dev_endowed_evm_accounts() -> BTreeMap<H160, GenesisAccount>{
  let endowed_account = vec![
    H160::from_str("6be02d1d3665660d22ff9624b7be0551ee1ac91b").unwrap(),
    H160::from_str("e6206C7f064c7d77C6d8e3eD8601c9AA435419cE").unwrap(),
    // the dev account key
    // seed: bottom drive obey lake curtain smoke basket hold race lonely fit walk
    // private key: 0x03183f27e9d78698a05c24eb6732630eb17725fcf2b53ee3a6a635d6ff139680
    H160::from_str("aed40f2261ba43b4dffe484265ce82d8ffe2b4db").unwrap()
  ];

  get_endowed_evm_accounts(endowed_account)
}

fn get_endowed_evm_accounts(endowed_account: Vec<H160>) -> BTreeMap<H160, GenesisAccount>{
  let mut evm_accounts = BTreeMap::new();
  for account in endowed_account {
    evm_accounts.insert(
      account,
      GenesisAccount {
        nonce: U256::from(0),
        balance: U256::from(10_000_000 * DOLLARS),
        storage: Default::default(),
        code: vec![],
      },
    );
  }
  evm_accounts
}

pub fn development_config(id: ParaId) -> Result<ChainSpec, String> {
  let wasm_binary = WASM_BINARY.ok_or("Development wasm binary not available".to_string())?;

  Ok(ChainSpec::from_genesis(
    // Name
    "Development",
    // ID
    "dev",
    ChainType::Development,
    move || testnet_genesis(
      wasm_binary,
      // Sudo account
      get_account_id_from_seed::<sr25519::Public>("Alice"),
      // Pre-funded accounts
      vec![
        get_account_id_from_seed::<sr25519::Public>("Alice"),
        get_account_id_from_seed::<sr25519::Public>("Bob"),
        get_account_id_from_seed::<sr25519::Public>("Alice//stash"),
        get_account_id_from_seed::<sr25519::Public>("Bob//stash"),
      ],
      true,
      dev_endowed_evm_accounts(),
      id,
      ),
    // Bootnodes
    vec![],
    // Telemetry
    None,
    // Protocol ID
    None,
    // Properties
    Some(json!({
      "tokenDecimals": 18,
      "tokenSymbol": "RCLV"
    }).as_object().expect("Created an object").clone()),
    // Extensions
    Extensions {
			relay_chain: "westend-dev".into(),
			para_id: id.into(),
		},
  ))
}

pub fn local_testnet_config(id: ParaId) -> Result<ChainSpec, String> {
  let wasm_binary = WASM_BINARY.ok_or("Development wasm binary not available".to_string())?;

  Ok(ChainSpec::from_genesis(
    // Name
    "Clover",
    // ID
    "local_testnet",
    ChainType::Local,
    move || testnet_genesis(
      wasm_binary,
      // Sudo account
      get_account_id_from_seed::<sr25519::Public>("Alice"),
      // Pre-funded accounts
      vec![
        get_account_id_from_seed::<sr25519::Public>("Alice"),
        get_account_id_from_seed::<sr25519::Public>("Bob"),
        get_account_id_from_seed::<sr25519::Public>("Charlie"),
        get_account_id_from_seed::<sr25519::Public>("Dave"),
        get_account_id_from_seed::<sr25519::Public>("Eve"),
        get_account_id_from_seed::<sr25519::Public>("Ferdie"),
        get_account_id_from_seed::<sr25519::Public>("Alice//stash"),
        get_account_id_from_seed::<sr25519::Public>("Bob//stash"),
        get_account_id_from_seed::<sr25519::Public>("Charlie//stash"),
        get_account_id_from_seed::<sr25519::Public>("Dave//stash"),
        get_account_id_from_seed::<sr25519::Public>("Eve//stash"),
        get_account_id_from_seed::<sr25519::Public>("Ferdie//stash"),
      ],
      true,
      endowed_evm_account(),
      id,
    ),
    // Bootnodes
    vec![],
    // Telemetry
    None,
    // Protocol ID
    None,
    // Properties
    Some(json!({
      "tokenDecimals": 18,
      "tokenSymbol": "RCLV"
    }).as_object().expect("Created an object").clone()),
    // Extensions
    Extensions {
			relay_chain: "westend-dev".into(),
			para_id: id.into(),
		},
  ))
}

pub fn local_rose_testnet_config(id: ParaId) -> Result<ChainSpec, String> {
  let wasm_binary = WASM_BINARY.ok_or("Development wasm binary not available".to_string())?;

  Ok(ChainSpec::from_genesis(
    // Name
    "Clover",
    // ID
    "clover-rococo-cc1",
    ChainType::Custom(String::from("rose")),
    move || testnet_genesis(
      wasm_binary,
      // rootkey: 5Cwo46bWWxaZCJQYkwH62nChaiEDKY9Kh4oo8kfbS9SNesMf
      hex!["26f702ab9792cbb2ea9c23b9f7982b6f6d6e9c3561e701175f9df919cf75f01f"].into(),
      // Pre-funded accounts
      vec![
        // 5Cwo46bWWxaZCJQYkwH62nChaiEDKY9Kh4oo8kfbS9SNesMf
        hex!["26f702ab9792cbb2ea9c23b9f7982b6f6d6e9c3561e701175f9df919cf75f01f"].into(),
      ],
      true,
      endowed_evm_account(),
      id,
    ),
    // Bootnodes
    vec![],
    // Telemetry
    TelemetryEndpoints::new(vec![(TELEMETRY_URL.into(), 0)]).ok(),
    // Protocol ID
    None,
    // Properties
    Some(json!({
      "tokenDecimals": 18,
      "tokenSymbol": "RCLV"
    }).as_object().expect("Created an object").clone()),
    // Extensions
    Extensions {
			relay_chain: "rococo".into(),
			para_id: 229_u32.into(),
		},
  ))
}

/// Configure initial storage state for FRAME modules.
fn testnet_genesis(
  wasm_binary: &[u8],
  // initial_authorities: Vec<(AccountId, AccountId, BabeId, GrandpaId, ImOnlineId)>,
  root_key: AccountId,
  endowed_accounts: Vec<AccountId>,
  _enable_println: bool,
  endowed_eth_accounts: BTreeMap<H160, GenesisAccount>,
  id: ParaId,
) -> GenesisConfig {
  let enable_println = true;

  const ENDOWMENT: Balance = 10_000_000 * DOLLARS;

  GenesisConfig {
    frame_system: SystemConfig {
      // Add Wasm runtime to storage.
      code: wasm_binary.to_vec(),
      changes_trie_config: Default::default(),
    },
    pallet_balances: BalancesConfig {
      // Configure endowed accounts with initial balance of 1 << 60.
      balances: endowed_accounts.iter().cloned()
            .map(|k| (k, ENDOWMENT))
            .collect(),
    },
    pallet_contracts: ContractsConfig {
      current_schedule: pallet_contracts::Schedule {
        enable_println, // this should only be enabled on development chains
        ..Default::default()
      },
    },
    clover_evm: EVMConfig {
      accounts: endowed_eth_accounts,
    },
    clover_ethereum: EthereumConfig {},
    pallet_indices: IndicesConfig {
      indices: vec![],
    },
    pallet_sudo: SudoConfig {
      // Assign network admin rights.
      key: root_key,
    },
    parachain_info: clover_runtime::ParachainInfoConfig { parachain_id: id },
    orml_oracle_Instance1: CloverOracleConfig {
      members: Default::default(), // initialized by OperatorMembership
      phantom: Default::default(),
    },
    orml_oracle_Instance2: BandOracleConfig {
      members: Default::default(), // initialized by OperatorMembership
      phantom: Default::default(),
    },
      orml_tokens: TokensConfig {
      endowed_accounts: endowed_accounts
        .iter()
        .flat_map(|x| {
          vec![
            (x.clone(), CurrencyId::CETH, 100000 * DOLLARS),
            (x.clone(), CurrencyId::CUSDT, 100000 * DOLLARS),
            (x.clone(), CurrencyId::DOT, 100000 * DOLLARS),
          ]
        })
        .collect(),
    },
    clover_incentives: IncentivesConfig{
      dex_rewards: vec![
        (CurrencyId::CLV, CurrencyId::DOT, 1 * DOLLARS),
        (CurrencyId::CLV, CurrencyId::CUSDT, 2 * DOLLARS),
        (CurrencyId::CETH, CurrencyId::CUSDT, 3 * DOLLARS),
      ],
    },
    cloverdex: CloverDexConfig {
        initial_pairs: vec![
          (CurrencyId::CUSDT, CurrencyId::CETH, Some(1000 * DOLLARS), Some(500 * DOLLARS)),
          (CurrencyId::CUSDT, CurrencyId::DOT, Some(700 * DOLLARS), Some(250 * DOLLARS)),
          (CurrencyId::CUSDT, CurrencyId::CLV, Some(300 * DOLLARS), Some(600 * DOLLARS)),
        ],
    },
    pallet_collective_Instance1: Default::default(),
    pallet_collective_Instance2: Default::default(),
    pallet_democracy: Default::default(),
    pallet_treasury: Default::default(),
    pallet_elections_phragmen: Default::default(),
    pallet_membership_Instance1: Default::default(),
  }
}
