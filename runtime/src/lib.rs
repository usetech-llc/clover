#![cfg_attr(not(feature = "std"), no_std)]
// `construct_runtime!` does a lot of recursion and requires us to increase the limit to 256.
#![recursion_limit="256"]

// Make the WASM binary available.
#[cfg(feature = "std")]
include!(concat!(env!("OUT_DIR"), "/wasm_binary.rs"));

use codec::Decode;
use sp_std::{prelude::*, marker::PhantomData};
use sp_core::{
  crypto::KeyTypeId,
  OpaqueMetadata, U256, H160, H256
};
use sp_runtime::{
  ApplyExtrinsicResult, generic, create_runtime_str, FixedPointNumber, impl_opaque_keys,
  ModuleId, transaction_validity::{TransactionPriority, TransactionValidity, TransactionSource},
  DispatchResult, OpaqueExtrinsic,
};
pub use sp_runtime::{Perbill, Percent, Permill, Perquintill};
use sp_runtime::traits::{
  BlakeTwo256, Block as BlockT, Convert, SaturatedConversion,
  StaticLookup,
};
use enum_iterator::IntoEnumIterator;

use sp_api::impl_runtime_apis;

// XCM imports
use polkadot_parachain::primitives::Sibling;
use xcm::v0::{Junction, MultiLocation, NetworkId};
use xcm_builder::{
  AccountId32Aliases, CurrencyAdapter, LocationInverter, ParentIsDefault, RelayChainAsNative,
  SiblingParachainAsNative, SiblingParachainConvertsVia, SignedAccountId32AsNative,
  SovereignSignedViaLocation,
};
use xcm_executor::{
  traits::{IsConcrete, NativeAsset},
  Config, XcmExecutor,
};

pub use pallet_im_online::sr25519::AuthorityId as ImOnlineId;
use pallet_contracts::weights::WeightInfo;
pub use pallet_transaction_payment::{FeeDetails, Multiplier, TargetedFeeAdjustment, };
use sp_version::RuntimeVersion;
#[cfg(feature = "std")]
use sp_version::NativeVersion;
use sp_core::{u32_trait::{_1, _2, _4, _5}};

// A few exports that help ease life for downstream crates.
#[cfg(any(feature = "std", test))]
pub use sp_runtime::BuildStorage;

use orml_traits::{create_median_value_data_provider, MultiCurrency, DataFeeder};
use orml_currencies::{BasicCurrencyAdapter};

pub use pallet_timestamp::Call as TimestampCall;
pub use pallet_balances::Call as BalancesCall;
use frame_system::{EnsureRoot, EnsureOneOf, limits};
pub use frame_support::{
  construct_runtime, debug, parameter_types, StorageValue,
  traits::{Currency, KeyOwnerProofSystem, Randomness, LockIdentifier, FindAuthor, SplitTwoWays, U128CurrencyToVote},
  weights::{
    Weight, IdentityFee,
    constants::{BlockExecutionWeight, ExtrinsicBaseWeight, RocksDbWeight, WEIGHT_PER_SECOND},
    DispatchClass,
  },
  ConsensusEngineId
};
use codec::{Encode};
use clover_evm::{
  Account as EVMAccount, FeeCalculator,
  EnsureAddressTruncated, Runner,
};
use evm_accounts::EvmAddressMapping;
use fp_rpc::{TransactionStatus};
use orml_traits::parameter_type_with_key;

pub use primitives::{
  AccountId, AccountIndex, Amount, Balance, BlockNumber, CurrencyId, EraIndex, Hash, Index,
  Moment, Rate, Share, Signature, Price,
    currency::*,
};

pub use constants::{time::*, };

use clover_traits::incentive_ops::IncentiveOps;

mod weights;
mod constants;
mod mock;
mod tests;

/// Opaque types. These are used by the CLI to instantiate machinery that don't need to know
/// the specifics of the runtime. They can then be made to be agnostic over specific formats
/// of data like extrinsics, allowing for them to continue syncing the network through upgrades
/// to even the core data structures.
pub mod opaque {
  use super::*;

  pub use sp_runtime::OpaqueExtrinsic as UncheckedExtrinsic;

  /// Opaque block header type.
  pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
  /// Opaque block type.
  pub type Block = generic::Block<Header, UncheckedExtrinsic>;
  /// Opaque block identifier type.
  pub type BlockId = generic::BlockId<Block>;
}

impl_opaque_keys! {
  pub struct SessionKeys {
  }
}

pub const VERSION: RuntimeVersion = RuntimeVersion {
  spec_name: create_runtime_str!("clover-rococo"),
  impl_name: create_runtime_str!("clover-rococo"),
  authoring_version: 1,
  spec_version: 3,
  impl_version: 1,
  apis: RUNTIME_API_VERSIONS,
  transaction_version: 1,
};

pub const MILLISECS_PER_BLOCK: u64 = 6000;

pub const SLOT_DURATION: u64 = MILLISECS_PER_BLOCK;

// Time is measured by number of blocks.
pub const MINUTES: BlockNumber = 60_000 / (MILLISECS_PER_BLOCK as BlockNumber);
pub const HOURS: BlockNumber = MINUTES * 60;
pub const DAYS: BlockNumber = HOURS * 24;

#[derive(codec::Encode, codec::Decode)]
pub enum XCMPMessage<XAccountId, XBalance> {
  /// Transfer tokens to the given account from the Parachain account.
  TransferToken(XAccountId, XBalance),
}

/// The version information used to identify this runtime when compiled natively.
#[cfg(feature = "std")]
pub fn native_version() -> NativeVersion {
  NativeVersion {
    runtime_version: VERSION,
    can_author_with: Default::default(),
  }
}

pub const MAXIMUM_BLOCK_WEIGHT: Weight = 2 * WEIGHT_PER_SECOND;
pub const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);
pub const AVERAGE_ON_INITIALIZE_RATIO: Perbill = Perbill::from_perthousand(25);

parameter_types! {
  pub BlockLength: limits::BlockLength =
    limits::BlockLength::max_with_normal_ratio(5 * 1024 * 1024, NORMAL_DISPATCH_RATIO);

  pub const BlockHashCount: BlockNumber = 2400;
  /// We allow for 2 seconds of compute with a 6 second average block time.
  pub BlockWeights: limits::BlockWeights = limits::BlockWeights::builder()
    .base_block(BlockExecutionWeight::get())
    .for_class(DispatchClass::all(), |weights| {
      weights.base_extrinsic = ExtrinsicBaseWeight::get();
    })
    .for_class(DispatchClass::Normal, |weights| {
      weights.max_total = Some(NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT);
    })
    .for_class(DispatchClass::Operational, |weights| {
      weights.max_total = Some(MAXIMUM_BLOCK_WEIGHT);
      // Operational transactions have an extra reserved space, so that they
      // are included even if block reached `MAXIMUM_BLOCK_WEIGHT`.
      weights.reserved = Some(
        MAXIMUM_BLOCK_WEIGHT - NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT,
      );
    })
    .avg_block_initialization(AVERAGE_ON_INITIALIZE_RATIO)
    .build_or_panic();

  pub const AvailableBlockRatio: Perbill = Perbill::from_percent(75);
  pub const Version: RuntimeVersion = VERSION;
  pub const SS58Prefix: u8 = 42; // TODO: register it to Ss58AddressFormat::CloverAccount?
}

// Configure FRAME pallets to include in runtime.

impl frame_system::Config for Runtime {
  /// The basic call filter to use in dispatchable.
  type BaseCallFilter = ();
  /// The identifier used to distinguish between accounts.
  type AccountId = AccountId;
  /// The aggregated dispatch type that is available for extrinsics.
  type Call = Call;
  /// The lookup mechanism to get account ID from whatever is passed in dispatchers.
  type Lookup = Indices;
  /// The index type for storing how many extrinsics an account has signed.
  type Index = Index;
  /// The index type for blocks.
  type BlockNumber = BlockNumber;
  /// The type for hashing blocks and tries.
  type Hash = Hash;
  /// The hashing algorithm used.
  type Hashing = BlakeTwo256;
  /// The header type.
  type Header = generic::Header<BlockNumber, BlakeTwo256>;
  /// The ubiquitous event type.
  type Event = Event;
  /// The ubiquitous origin type.
  type Origin = Origin;
  /// Maximum number of block number to block hash mappings to keep (oldest pruned first).
  type BlockHashCount = BlockHashCount;
  type BlockWeights = BlockWeights;
  type BlockLength = BlockLength;
  /// The weight of database operations that the runtime can invoke.
  type DbWeight = RocksDbWeight;
  /// Version of the runtime.
  type Version = Version;
  type PalletInfo = PalletInfo;
  /// What to do if a new account is created.
  type OnNewAccount = ();
  /// What to do if an account is fully reaped from the system.
  type OnKilledAccount = (
    clover_evm::CallKillAccount<Runtime>,
    evm_accounts::CallKillAccount<Runtime>,
  );
  /// The data to be stored in an account.
  type AccountData = pallet_balances::AccountData<Balance>;
  /// Weight information for the extrinsics of this pallet.
  type SystemWeightInfo = ();
  type SS58Prefix = SS58Prefix;
}

parameter_types! {
  pub const MinimumPeriod: u64 = SLOT_DURATION / 2;
}

impl pallet_timestamp::Config for Runtime {
  /// A timestamp: milliseconds since the unix epoch.
  type Moment = u64;
  type OnTimestampSet = ();
  type MinimumPeriod = MinimumPeriod;
  type WeightInfo = ();
}

/// clover account
impl evm_accounts::Config for Runtime {
  type Event = Event;
  type Currency = Balances;
  type KillAccount = frame_system::Consumer<Runtime>;
  type AddressMapping = EvmAddressMapping<Runtime>;
  type MergeAccount = Currencies;
  type WeightInfo = weights::evm_accounts::WeightInfo<Runtime>;
}

impl evm_bridge::Config for Runtime {
  type EVM = Ethereum;
}

/// clover evm
pub struct FixedGasPrice;

impl FeeCalculator for FixedGasPrice {
  fn min_gas_price() -> U256 {
    1_000_000_000.into()
  }
}

parameter_types! {
  pub const ChainId: u64 = 1337;
}

impl clover_evm::Config for Runtime {
  type FeeCalculator = FixedGasPrice;
  type GasToWeight = ();
  type CallOrigin = EnsureAddressTruncated;
  type WithdrawOrigin = EnsureAddressTruncated;
  type AddressMapping = EvmAddressMapping<Runtime>;
  type Currency = Balances;
  type MergeAccount = Currencies;
  type Event = Event;
  type Runner = clover_evm::runner::stack::Runner<Self>;
  type Precompiles = (
    clover_evm::precompiles::ECRecover,
    clover_evm::precompiles::Sha256,
    clover_evm::precompiles::Ripemd160,
    clover_evm::precompiles::Identity,
  );
  type ChainId = ChainId;
}

pub struct EthereumFindAuthor<F>(PhantomData<F>);

impl<F: FindAuthor<u32>> FindAuthor<H160> for EthereumFindAuthor<F> {
  fn find_author<'a, I>(_digests: I) -> Option<H160>
  where
    I: 'a + IntoIterator<Item = (ConsensusEngineId, &'a [u8])>,
  {
    None
  }
}

/// parachain doesn't have an authorship support currently
pub struct PhantomMockAuthorship;

impl FindAuthor<u32> for PhantomMockAuthorship{
  fn find_author<'a, I>(_digests: I) -> Option<u32>
  where
    I: 'a + IntoIterator<Item = (ConsensusEngineId, &'a [u8])>,
  {
    Some(0 as u32)
  }
}

impl clover_ethereum::Config for Runtime {
  type Event = Event;
  type FindAuthor = EthereumFindAuthor<PhantomMockAuthorship>;
}

pub struct TransactionConverter;

impl fp_rpc::ConvertTransaction<UncheckedExtrinsic> for TransactionConverter {
  fn convert_transaction(&self, transaction: clover_ethereum::Transaction) -> UncheckedExtrinsic {
    UncheckedExtrinsic::new_unsigned(clover_ethereum::Call::<Runtime>::transact(transaction).into())
  }
}

impl fp_rpc::ConvertTransaction<OpaqueExtrinsic> for TransactionConverter {
  fn convert_transaction(&self, transaction: clover_ethereum::Transaction) -> OpaqueExtrinsic {
    let extrinsic =
        UncheckedExtrinsic::new_unsigned(clover_ethereum::Call::<Runtime>::transact(transaction).into());
    let encoded = extrinsic.encode();
    OpaqueExtrinsic::decode(&mut &encoded[..]).expect("Encoded extrinsic is always valid")
  }
}

/// Struct that handles the conversion of Balance -> `u64`. This is used for
/// staking's election calculation.
pub struct CurrencyToVoteHandler;

impl CurrencyToVoteHandler {
}

impl Convert<u64, u64> for CurrencyToVoteHandler {
  fn convert(x: u64) -> u64 {
      x
  }
}
impl Convert<u128, u128> for CurrencyToVoteHandler {
  fn convert(x: u128) -> u128 {
      x
  }
}
impl Convert<u128, u64> for CurrencyToVoteHandler {
  fn convert(x: u128) -> u64 {
      x.saturated_into()
  }
}

impl Convert<u64, u128> for CurrencyToVoteHandler {
  fn convert(x: u64) -> u128 {
      x as u128
  }
}


parameter_types! {
  pub const ExistentialDeposit: u128 = 500;
  pub const MaxLocks: u32 = 50;
}

pub type NegativeImbalance<T> = <pallet_balances::Module<T> as Currency<<T as frame_system::Config>::AccountId>>::NegativeImbalance;

impl pallet_balances::Config for Runtime {
  /// The type for recording an account's balance.
  type Balance = Balance;
  /// The ubiquitous event type.
  type Event = Event;
  type DustRemoval = ();
  type ExistentialDeposit = ExistentialDeposit;
  type AccountStore = System;
  type MaxLocks = MaxLocks;
  type WeightInfo = ();
}

parameter_types! {
  pub const SessionDuration: BlockNumber = EPOCH_DURATION_IN_BLOCKS as _;
  pub const ImOnlineUnsignedPriority: TransactionPriority = TransactionPriority::max_value();
}

parameter_types! {
  pub OffencesWeightSoftLimit: Weight = Perbill::from_percent(60) * MAXIMUM_BLOCK_WEIGHT;
}

parameter_types! {
  pub MaximumSchedulerWeight: Weight = Perbill::from_percent(10) * MAXIMUM_BLOCK_WEIGHT;
  pub const MaxScheduledPerBlock: u32 = 50;
}

// democracy
impl pallet_scheduler::Config for Runtime {
  type Event = Event;
  type Origin = Origin;
  type Call = Call;
  type MaximumWeight = MaximumSchedulerWeight;
  type PalletsOrigin = OriginCaller;
  type ScheduleOrigin = EnsureRoot<AccountId>;
  type MaxScheduledPerBlock = MaxScheduledPerBlock;
  type WeightInfo = ();
}

parameter_types! {
  pub const LaunchPeriod: BlockNumber = 7 * MINUTES;
  pub const VotingPeriod: BlockNumber = 7 * MINUTES;
  pub const FastTrackVotingPeriod: BlockNumber = 1 * MINUTES;
  pub const MinimumDeposit: Balance = 100 * DOLLARS;
  pub const EnactmentPeriod: BlockNumber = 8 * MINUTES;
  pub const CooloffPeriod: BlockNumber = 7 * MINUTES;
  // One cent: $10,000 / MB
  pub const PreimageByteDeposit: Balance = 10 * MILLICENTS;
  pub const InstantAllowed: bool = false;
  pub const MaxVotes: u32 = 100;
  pub const MaxProposals: u32 = 100;
}

impl pallet_democracy::Config for Runtime {
  type Proposal = Call;
  type Event = Event;
  type Currency = Balances;
  type EnactmentPeriod = EnactmentPeriod;
  type LaunchPeriod = LaunchPeriod;
  type VotingPeriod = VotingPeriod;
  type MinimumDeposit = MinimumDeposit;
  /// A straight majority of the council can decide what their next motion is.
  type ExternalOrigin = pallet_collective::EnsureProportionMoreThan<_1, _2, AccountId, CouncilCollective>;
  /// A super-majority can have the next scheduled referendum be a straight
  /// majority-carries vote.
  type ExternalMajorityOrigin = pallet_collective::EnsureProportionAtLeast<_4, _5, AccountId, CouncilCollective>;
  /// A unanimous council can have the next scheduled referendum be a straight
  /// default-carries (NTB) vote.
  type ExternalDefaultOrigin = pallet_collective::EnsureProportionAtLeast<_1, _1, AccountId, CouncilCollective>;
  /// Full of the technical committee can have an
  /// ExternalMajority/ExternalDefault vote be tabled immediately and with a
  /// shorter voting/enactment period.
  type FastTrackOrigin = pallet_collective::EnsureProportionAtLeast<_1, _1, AccountId, TechnicalCollective>;
  type InstantOrigin = frame_system::EnsureNever<AccountId>;
  type InstantAllowed = InstantAllowed;
  type FastTrackVotingPeriod = FastTrackVotingPeriod;
  /// To cancel a proposal which has been passed, all of the council must
  /// agree to it.
  type CancellationOrigin = pallet_collective::EnsureProportionAtLeast<_1, _1, AccountId, CouncilCollective>;
  type OperationalPreimageOrigin = pallet_collective::EnsureMember<AccountId, CouncilCollective>;
  type BlacklistOrigin = EnsureRoot<AccountId>;
  type CancelProposalOrigin = EnsureOneOf<
        AccountId,
        EnsureRoot<AccountId>,
        pallet_collective::EnsureProportionAtLeast<_1, _1, AccountId, TechnicalCollective>,
    >;
  /// Any single technical committee member may veto a coming council
  /// proposal, however they can only do it once and it lasts only for the
  /// cooloff period.
  type VetoOrigin = pallet_collective::EnsureMember<AccountId, TechnicalCollective>;
  type CooloffPeriod = CooloffPeriod;
  type PreimageByteDeposit = PreimageByteDeposit;
  type Slash = Treasury;
  type Scheduler = Scheduler;
  type MaxVotes = MaxVotes;
  type MaxProposals = MaxProposals;
  type PalletsOrigin = OriginCaller;
  type WeightInfo = ();
}

impl pallet_utility::Config for Runtime {
  type Event = Event;
  type Call = Call;
  type WeightInfo = ();
}

parameter_types! {
  pub const CouncilMotionDuration: BlockNumber = 3 * DAYS;
  pub const CouncilMaxProposals: u32 = 100;
  pub const GeneralCouncilMaxMembers: u32 = 100;
}

type CouncilCollective = pallet_collective::Instance1;
impl pallet_collective::Config<CouncilCollective> for Runtime {
  type Origin = Origin;
  type Proposal = Call;
  type Event = Event;
  type MotionDuration = CouncilMotionDuration;
  type MaxProposals = CouncilMaxProposals;
  type MaxMembers = GeneralCouncilMaxMembers;
  type DefaultVote = pallet_collective::PrimeDefaultVote;
  type WeightInfo = ();
}

/// Converter for currencies to votes.
pub struct CurrencyToVoteHandler2<R>(sp_std::marker::PhantomData<R>);

impl<R> CurrencyToVoteHandler2<R>
where
  R: pallet_balances::Config,
  R::Balance: Into<u128>,
{
  fn factor() -> u128 {
    let issuance: u128 = <pallet_balances::Module<R>>::total_issuance().into();
    (issuance / u64::max_value() as u128).max(1)
  }
}

impl<R> Convert<u128, u64> for CurrencyToVoteHandler2<R>
where
  R: pallet_balances::Config,
  R::Balance: Into<u128>,
{
  fn convert(x: u128) -> u64 { (x / Self::factor()) as u64 }
}

impl<R> Convert<u128, u128> for CurrencyToVoteHandler2<R>
where
  R: pallet_balances::Config,
  R::Balance: Into<u128>,
{
  fn convert(x: u128) -> u128 { x * Self::factor() }
}

parameter_types! {
  pub const CandidacyBond: Balance = 1 * DOLLARS;
    // 1 storage item created, key size is 32 bytes, value size is 16+16.
  pub const VotingBondBase: Balance = deposit(1, 64);
  // additional data per vote is 32 bytes (account id).
  pub const VotingBondFactor: Balance = deposit(0, 32);
  /// Daily council elections.
  pub const TermDuration: BlockNumber = 24 * HOURS;
  pub const DesiredMembers: u32 = 17;
  pub const DesiredRunnersUp: u32 = 30;
  pub const ElectionsPhragmenModuleId: LockIdentifier = *b"phrelect";
}

impl pallet_elections_phragmen::Config for Runtime {
  type Event = Event;
  type Currency = Balances;
  type ChangeMembers = Council;
  type InitializeMembers = Council;
  type CurrencyToVote = U128CurrencyToVote;
  type CandidacyBond = CandidacyBond;
  type VotingBondBase = VotingBondBase;
  type VotingBondFactor = VotingBondFactor;
  type LoserCandidate = Treasury;
  type KickedMember = Treasury;
  type DesiredMembers = DesiredMembers;
  type DesiredRunnersUp = DesiredRunnersUp;
  type TermDuration = TermDuration;
  type ModuleId = ElectionsPhragmenModuleId;
  type WeightInfo = ();
}

parameter_types! {
  pub const TechnicalMotionDuration: BlockNumber = 3 * DAYS;
  pub const TechnicalMaxProposals: u32 = 100;
  pub const TechnicalMaxMembers:u32 = 100;
}

type TechnicalCollective = pallet_collective::Instance2;
impl pallet_collective::Config<TechnicalCollective> for Runtime {
  type Origin = Origin;
  type Proposal = Call;
  type Event = Event;
  type MotionDuration = TechnicalMotionDuration;
  type MaxProposals = TechnicalMaxProposals;
  type MaxMembers = TechnicalMaxMembers;
  type DefaultVote = pallet_collective::PrimeDefaultVote;
  type WeightInfo = ();
}

impl pallet_membership::Config<pallet_membership::Instance1> for Runtime {
  type Event = Event;
  type AddOrigin = frame_system::EnsureRoot<AccountId>;
  type RemoveOrigin = frame_system::EnsureRoot<AccountId>;
  type SwapOrigin = frame_system::EnsureRoot<AccountId>;
  type ResetOrigin = frame_system::EnsureRoot<AccountId>;
  type PrimeOrigin = frame_system::EnsureRoot<AccountId>;
  type MembershipInitialized = TechnicalCommittee;
  type MembershipChanged = TechnicalCommittee;
}

parameter_types! {
  pub const ProposalBond: Permill = Permill::from_percent(5);
  pub const ProposalBondMinimum: Balance = 20 * DOLLARS;
  pub const SpendPeriod: BlockNumber = 6 * DAYS;
  pub const Burn: Permill = Permill::from_percent(1);
  pub const TreasuryModuleId: ModuleId = ModuleId(*b"py/trsry");

  pub const TipCountdown: BlockNumber = 1 * DAYS;
  pub const TipFindersFee: Percent = Percent::from_percent(20);
  pub const TipReportDepositBase: Balance = 1 * DOLLARS;
  pub const DataDepositPerByte: Balance = 10 * MILLICENTS;
  pub const BountyDepositBase: Balance = DOLLARS;
  pub const BountyDepositPayoutDelay: BlockNumber = DAYS;
  pub const BountyUpdatePeriod: BlockNumber = 14 * DAYS;
  pub const BountyCuratorDeposit: Permill = Permill::from_percent(50);
  pub const BountyValueMinimum: Balance = 5 * DOLLARS;
  pub const MaximumReasonLength: u32 = 16384;
}

impl pallet_treasury::Config for Runtime {
  type Currency = Balances;
  type ApproveOrigin = pallet_collective::EnsureProportionMoreThan<_1, _2, AccountId, CouncilCollective>;
  type RejectOrigin = pallet_collective::EnsureProportionMoreThan<_1, _5, AccountId, CouncilCollective>;
  type Event = Event;
  type OnSlash = Treasury;
  type ProposalBond = ProposalBond;
  type ProposalBondMinimum = ProposalBondMinimum;
  type SpendPeriod = SpendPeriod;
  type SpendFunds = ();
  type Burn = Burn;
  type BurnDestination = ();
  type ModuleId = TreasuryModuleId;
  type WeightInfo = ();
}

parameter_types! {
  pub const TransactionBaseFee: Balance = 1 * CENTS;
  pub const TransactionByteFee: Balance = 10 * MILLICENTS;
  pub const TargetBlockFullness: Perquintill = Perquintill::from_percent(25);
  pub AdjustmentVariable: Multiplier = Multiplier::saturating_from_rational(1, 100_000);
  pub MinimumMultiplier: Multiplier = Multiplier::saturating_from_rational(1, 1_000_000_000u128);
}

impl pallet_transaction_payment::Config for Runtime {
  type OnChargeTransaction = pallet_transaction_payment::CurrencyAdapter<Balances, ()>;
  type TransactionByteFee = TransactionByteFee;
  type WeightToFee = IdentityFee<Balance>;
  type FeeMultiplierUpdate =
  TargetedFeeAdjustment<Self, TargetBlockFullness, AdjustmentVariable, MinimumMultiplier>;
}

impl pallet_sudo::Config for Runtime {
  type Event = Event;
  type Call = Call;
}

parameter_types! {
  pub const IndexDeposit: Balance = 1 * DOLLARS;
}

impl pallet_indices::Config for Runtime {
  type AccountIndex = AccountIndex;
  type Event = Event;
  type Currency = Balances;
  type Deposit = IndexDeposit;
  type WeightInfo = ();
}

impl<LocalCall> frame_system::offchain::CreateSignedTransaction<LocalCall> for Runtime
where
  Call: From<LocalCall>,
{
  fn create_transaction<C: frame_system::offchain::AppCrypto<Self::Public, Self::Signature>>(
    call: Call,
    public: <Signature as sp_runtime::traits::Verify>::Signer,
    account: AccountId,
    nonce: Index,
  ) -> Option<(
    Call,
    <UncheckedExtrinsic as sp_runtime::traits::Extrinsic>::SignaturePayload,
  )> {
    // take the biggest period possible.
    let period = BlockHashCount::get()
      .checked_next_power_of_two()
      .map(|c| c / 2)
      .unwrap_or(2) as u64;
    let current_block = System::block_number()
      .saturated_into::<u64>()
      // The `System::block_number` is initialized with `n+1`,
      // so the actual block number is `n`.
      .saturating_sub(1);
    let tip = 0;
    let extra: SignedExtra = (
      frame_system::CheckSpecVersion::<Runtime>::new(),
      frame_system::CheckTxVersion::<Runtime>::new(),
      frame_system::CheckGenesis::<Runtime>::new(),
      frame_system::CheckEra::<Runtime>::from(generic::Era::mortal(period, current_block)),
      frame_system::CheckNonce::<Runtime>::from(nonce),
      frame_system::CheckWeight::<Runtime>::new(),
      pallet_transaction_payment::ChargeTransactionPayment::<Runtime>::from(tip),
    );
    let raw_payload = SignedPayload::new(call, extra)
      .map_err(|e| {
        // debug::warn!("Unable to create signed payload: {:?}", e);
      })
      .ok()?;
    let signature = raw_payload.using_encoded(|payload| C::sign(payload, public))?;
    let address = Indices::unlookup(account);
    let (call, extra, _) = raw_payload.deconstruct();
    Some((call, (address, signature, extra)))
  }
}

impl frame_system::offchain::SigningTypes for Runtime {
  type Public = <Signature as sp_runtime::traits::Verify>::Signer;
  type Signature = Signature;
}

impl<C> frame_system::offchain::SendTransactionTypes<C> for Runtime
where
  Call: From<C>,
{
  type OverarchingCall = Call;
  type Extrinsic = UncheckedExtrinsic;
}

parameter_type_with_key! {
  pub ExistentialDeposits: |currency_id: CurrencyId| -> Balance {
    Default::default()
  };
}

impl orml_tokens::Config for Runtime {
  type Event = Event;
  type Balance = Balance;
  type Amount = Amount;
  type CurrencyId = CurrencyId;
  type WeightInfo = ();
  type ExistentialDeposits = ExistentialDeposits;
  type OnDust = ();
}

parameter_types! {
  pub const GetNativeCurrencyId: CurrencyId = CurrencyId::CLV;
}

impl orml_currencies::Config for Runtime {
  type Event = Event;
  type MultiCurrency = Tokens;
  type NativeCurrency = BasicCurrencyAdapter<Runtime, Balances, Amount, BlockNumber>;
  type GetNativeCurrencyId = GetNativeCurrencyId;
  type WeightInfo = ();
}

parameter_types! {
  pub const RewardModuleId: ModuleId = ModuleId(*b"clv/repm");
  pub const ExistentialReward: u128 = 100;
}

impl reward_pool::Config for Runtime {
  type Event = Event;
  type PoolId = clover_incentives::PoolId;
  type ModuleId = RewardModuleId;
  type Currency = Currencies;
  type GetNativeCurrencyId = GetNativeCurrencyId;
  type ExistentialReward = ExistentialReward;
  type Handler = Incentives;
}

impl clover_incentives::Config for Runtime {
  type RewardPool = RewardPool;
}

parameter_types! {
  pub GetExchangeFee: Rate = Rate::saturating_from_rational(1, 1000);
  pub const CloverdexModuleId: ModuleId = ModuleId(*b"clv/dexm");
}

impl cloverdex::Config for Runtime {
  type Event = Event;
  type Currency = Currencies;
  type Share = Share;
  type GetExchangeFee = GetExchangeFee;
  type ModuleId = CloverdexModuleId;
  type OnAddLiquidity = ();
  type OnRemoveLiquidity = ();
  type IncentiveOps = Incentives;
}

parameter_types! {
  pub const LoansModuleId: ModuleId = ModuleId(*b"clv/loan");
}

impl clover_loans::Config for Runtime {
  type Event = Event;
  type Currency = Currencies;
  type ModuleId = LoansModuleId;
}

type CloverDataProvider = orml_oracle::Instance1;
impl orml_oracle::Config<CloverDataProvider> for Runtime {
  type Event = Event;
  type OnNewData = ();
  type CombineData = orml_oracle::DefaultCombineData<Runtime, MinimumCount, ExpiresIn, CloverDataProvider>;
  type Time = Timestamp;
  type OracleKey = CurrencyId;
  type OracleValue = Price;
  type RootOperatorAccountId = ZeroAccountId;
  type WeightInfo = ();
}

type BandDataProvider = orml_oracle::Instance2;
impl orml_oracle::Config<BandDataProvider> for Runtime {
  type Event = Event;
  type OnNewData = ();
  type CombineData = orml_oracle::DefaultCombineData<Runtime, MinimumCount, ExpiresIn, BandDataProvider>;
  type Time = Timestamp;
  type OracleKey = CurrencyId;
  type OracleValue = Price;
  type RootOperatorAccountId = ZeroAccountId;
  type WeightInfo = ();
}

type TimeStampedPrice = orml_oracle::TimestampedValue<Price, primitives::Moment>;
create_median_value_data_provider!(
  AggregatedDataProvider,
  CurrencyId,
  Price,
  TimeStampedPrice,
  [CloverOracle, BandOracle]
);
// Aggregated data provider cannot feed.
impl DataFeeder<CurrencyId, Price, AccountId> for AggregatedDataProvider {
  fn feed_value(_: AccountId, _: CurrencyId, _: Price) -> DispatchResult {
    Err("Not supported".into())
  }
}

pub const fn deposit(items: u32, bytes: u32) -> Balance {
  items as Balance * 15 * CENTS + (bytes as Balance) * 6 * CENTS
}

parameter_types! {
  pub const TombstoneDeposit: Balance = 16 * MILLICENTS;
  pub const SurchargeReward: Balance = 150 * MILLICENTS;
  pub const SignedClaimHandicap: u32 = 2;
  pub const MaxDepth: u32 = 32;
  pub const MaxValueSize: u32 = 16 * 1024;
  pub const MaxCodeSize: u32 = 128 * 1024;
  pub const DepositPerContract: Balance = TombstoneDeposit::get();
  pub const DepositPerStorageByte: Balance = deposit(0, 1);
  pub const DepositPerStorageItem: Balance = deposit(1, 0);
  pub RentFraction: Perbill = Perbill::from_rational_approximation(1u32, 30 * DAYS);
  // The lazy deletion runs inside on_initialize.
  pub DeletionWeightLimit: Weight = AVERAGE_ON_INITIALIZE_RATIO *
    BlockWeights::get().max_block;
  // The weight needed for decoding the queue should be less or equal than a fifth
  // of the overall weight dedicated to the lazy deletion.
  pub DeletionQueueDepth: u32 = ((DeletionWeightLimit::get() / (
      <Runtime as pallet_contracts::Config>::WeightInfo::on_initialize_per_queue_item(1) -
      <Runtime as pallet_contracts::Config>::WeightInfo::on_initialize_per_queue_item(0)
    )) / 5) as u32;
}

impl pallet_contracts::Config for Runtime {
  type Time = Timestamp;
  type Randomness = RandomnessCollectiveFlip;
  type Currency = Balances;
  type Event = Event;
  type RentPayment = ();
  type SignedClaimHandicap = SignedClaimHandicap;
  type TombstoneDeposit = TombstoneDeposit;
  type DepositPerContract = DepositPerContract;
  type DepositPerStorageByte = DepositPerStorageByte;
  type DepositPerStorageItem = DepositPerStorageItem;
  type RentFraction = RentFraction;
  type SurchargeReward = SurchargeReward;
  type MaxDepth = MaxDepth;
  type MaxValueSize = MaxValueSize;
  type WeightPrice = pallet_transaction_payment::Module<Self>;
  type WeightInfo = pallet_contracts::weights::SubstrateWeight<Self>;
  type ChainExtension = ();
  type DeletionQueueDepth = DeletionQueueDepth;
  type DeletionWeightLimit = DeletionWeightLimit;
  type MaxCodeSize = MaxCodeSize;
}

parameter_types! {
  pub const GetStableCurrencyId: CurrencyId = CurrencyId::CUSDT;
  pub StableCurrencyFixedPrice: Price = Price::saturating_from_rational(1, 1);
  pub const MinimumCount: u32 = 1;
  pub const ExpiresIn: Moment = 1000 * 60 * 60; // 60 mins
  pub ZeroAccountId: AccountId = AccountId::from([0u8; 32]);
}

type EnsureRootOrHalfGeneralCouncil = EnsureOneOf<
  AccountId,
  EnsureRoot<AccountId>,
  pallet_collective::EnsureProportionMoreThan<_1, _2, AccountId, CouncilCollective>,
>;

impl clover_prices::Config for Runtime {
  type Event = Event;
  type Source = AggregatedDataProvider;
  type GetStableCurrencyId = GetStableCurrencyId;
  type StableCurrencyFixedPrice = StableCurrencyFixedPrice;
  type LockOrigin = EnsureRootOrHalfGeneralCouncil;
}

impl cumulus_pallet_parachain_system::Config for Runtime {
  type Event = Event;
  type OnValidationData = ();
  type SelfParaId = parachain_info::Module<Runtime>;
  type DownwardMessageHandlers = ();
  type HrmpMessageHandlers = ();
}

impl parachain_info::Config for Runtime {}

parameter_types! {
  pub const RococoLocation: MultiLocation = MultiLocation::X1(Junction::Parent);
  pub const RococoNetwork: NetworkId = NetworkId::Polkadot;
  pub RelayChainOrigin: Origin = cumulus_pallet_xcm_handler::Origin::Relay.into();
  pub Ancestry: MultiLocation = Junction::Parachain {
    id: ParachainInfo::parachain_id().into()
  }.into();
}

type LocationConverter = (
  ParentIsDefault<AccountId>,
  SiblingParachainConvertsVia<Sibling, AccountId>,
  AccountId32Aliases<RococoNetwork, AccountId>,
);

type LocalAssetTransactor = CurrencyAdapter<
  // Use this currency:
  Balances,
  // Use this currency when it is a fungible asset matching the given location or name:
  IsConcrete<RococoLocation>,
  // Do a simple punn to convert an AccountId32 MultiLocation into a native chain account ID:
  LocationConverter,
  // Our chain's account ID type (we can't get away without mentioning it explicitly):
  AccountId,
>;

type LocalOriginConverter = (
  SovereignSignedViaLocation<LocationConverter, Origin>,
  RelayChainAsNative<RelayChainOrigin, Origin>,
  SiblingParachainAsNative<cumulus_pallet_xcm_handler::Origin, Origin>,
  SignedAccountId32AsNative<RococoNetwork, Origin>,
);

pub struct XcmConfig;
impl Config for XcmConfig {
  type Call = Call;
  type XcmSender = XcmHandler;
  // How to withdraw and deposit an asset.
  type AssetTransactor = LocalAssetTransactor;
  type OriginConverter = LocalOriginConverter;
  type IsReserve = NativeAsset;
  type IsTeleporter = ();
  type LocationInverter = LocationInverter<Ancestry>;
}

impl cumulus_pallet_xcm_handler::Config for Runtime {
  type Event = Event;
  type XcmExecutor = XcmExecutor<XcmConfig>;
  type UpwardMessageSender = ParachainSystem;
  type HrmpMessageSender = ParachainSystem;
  type SendXcmOrigin = EnsureRoot<AccountId>;
  type AccountIdConverter = LocationConverter;
}

// Create the runtime by composing the FRAME pallets that were previously configured.
construct_runtime!(
  pub enum Runtime where
    Block = Block,
    NodeBlock = opaque::Block,
    UncheckedExtrinsic = UncheckedExtrinsic
  {
    System: frame_system::{Pallet, Call, Config, Storage, Event<T>},
    RandomnessCollectiveFlip: pallet_randomness_collective_flip::{Pallet, Call, Storage},
    Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},

    Indices: pallet_indices::{Pallet, Call, Storage, Config<T>, Event<T>},
    Balances: pallet_balances::{Pallet, Call, Storage, Config<T>, Event<T>},

    ParachainSystem: cumulus_pallet_parachain_system::{Pallet, Call, Storage, Inherent, Event},

    TransactionPayment: pallet_transaction_payment::{Pallet, Storage},

    ParachainInfo: parachain_info::{Pallet, Storage, Config},
    XcmHandler: cumulus_pallet_xcm_handler::{Pallet, Event<T>, Origin},

    Currencies: orml_currencies::{Pallet, Call, Event<T>},
    Tokens: orml_tokens::{Pallet, Storage, Event<T>, Config<T>},

    // Governance.
    Democracy: pallet_democracy::{Pallet, Call, Storage, Config, Event<T>},
    Council: pallet_collective::<Instance1>::{Pallet, Call, Storage, Origin<T>, Event<T>, Config<T>},
    TechnicalCommittee: pallet_collective::<Instance2>::{Pallet, Call, Storage, Origin<T>, Event<T>, Config<T>},
    ElectionsPhragmen: pallet_elections_phragmen::{Pallet, Call, Storage, Event<T>, Config<T>},
    //ElectionsPhragmen: pallet_elections_phragmen::{Module, Call, Storage, Event<T>},
    TechnicalMembership: pallet_membership::<Instance1>::{Pallet, Call, Storage, Event<T>, Config<T>},
    Treasury: pallet_treasury::{Pallet, Call, Storage, Event<T>, Config},

    // Clover module
    CloverDex: cloverdex::{Pallet, Storage, Call, Event<T>, Config},
    RewardPool: reward_pool::{Pallet, Storage, Call, Event<T>,},
    Incentives: clover_incentives::{Pallet, Storage, Call, Config},
    Prices: clover_prices::{Pallet, Storage, Call, Event},
    Loans: clover_loans::{Pallet, Storage, Call, Event<T>},

    // oracle
    CloverOracle: orml_oracle::<Instance1>::{Pallet, Storage, Call, Config<T>, Event<T>},
    BandOracle: orml_oracle::<Instance2>::{Pallet, Storage, Call, Config<T>, Event<T>},

    // Smart contracts modules
    Contracts: pallet_contracts::{Pallet, Call, Config<T>, Storage, Event<T>},
    EVM: clover_evm::{Pallet, Config, Call, Storage, Event<T>},
    Ethereum: clover_ethereum::{Pallet, Call, Storage, Event, Config, ValidateUnsigned},

    Sudo: pallet_sudo::{Pallet, Call, Config<T>, Storage, Event<T>},

    // Utility module.
    Scheduler: pallet_scheduler::{Pallet, Call, Storage, Event<T>},
    Utility: pallet_utility::{Pallet, Call, Event},

    // account module
    EvmAccounts: evm_accounts::{Pallet, Call, Storage, Event<T>},
    EVMBridge: evm_bridge::{Pallet},
  }
);

/// The address format for describing accounts.
pub type Address = sp_runtime::MultiAddress<AccountId, AccountIndex>;

/// Block header type as expected by this runtime.
pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
/// Block type as expected by this runtime.
pub type Block = generic::Block<Header, UncheckedExtrinsic>;
/// A Block signed with a Justification
pub type SignedBlock = generic::SignedBlock<Block>;
/// BlockId type as expected by this runtime.
pub type BlockId = generic::BlockId<Block>;
/// The SignedExtension to the basic transaction logic.
pub type SignedExtra = (
  frame_system::CheckSpecVersion<Runtime>,
  frame_system::CheckTxVersion<Runtime>,
  frame_system::CheckGenesis<Runtime>,
  frame_system::CheckEra<Runtime>,
  frame_system::CheckNonce<Runtime>,
  frame_system::CheckWeight<Runtime>,
  pallet_transaction_payment::ChargeTransactionPayment<Runtime>,
);
/// Unchecked extrinsic type as expected by this runtime.
pub type UncheckedExtrinsic = generic::UncheckedExtrinsic<Address, Call, Signature, SignedExtra>;
/// Extrinsic type that has already been checked.
pub type CheckedExtrinsic = generic::CheckedExtrinsic<AccountId, Call, SignedExtra>;
/// Executive: handles dispatch to the various modules.
pub type Executive = frame_executive::Executive<
  Runtime,
  Block,
  frame_system::ChainContext<Runtime>,
  Runtime,
  AllModules,
>;

pub type SignedPayload = generic::SignedPayload<Call, SignedExtra>;
impl_runtime_apis! {
  impl sp_api::Core<Block> for Runtime {
    fn version() -> RuntimeVersion {
      VERSION
    }

    fn execute_block(block: Block) {
      Executive::execute_block(block)
    }

    fn initialize_block(header: &<Block as BlockT>::Header) {
      Executive::initialize_block(header)
    }
  }

  impl sp_api::Metadata<Block> for Runtime {
    fn metadata() -> OpaqueMetadata {
      Runtime::metadata().into()
    }
  }

  impl sp_block_builder::BlockBuilder<Block> for Runtime {
    fn apply_extrinsic(extrinsic: <Block as BlockT>::Extrinsic) -> ApplyExtrinsicResult {
      Executive::apply_extrinsic(extrinsic)
    }

    fn finalize_block() -> <Block as BlockT>::Header {
      Executive::finalize_block()
    }

    fn inherent_extrinsics(data: sp_inherents::InherentData) -> Vec<<Block as BlockT>::Extrinsic> {
      data.create_extrinsics()
    }

    fn check_inherents(
      block: Block,
      data: sp_inherents::InherentData,
    ) -> sp_inherents::CheckInherentsResult {
      data.check_extrinsics(&block)
    }

    fn random_seed() -> <Block as BlockT>::Hash {
      RandomnessCollectiveFlip::random_seed().0
    }
  }

  impl sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block> for Runtime {
    fn validate_transaction(
      source: TransactionSource,
      tx: <Block as BlockT>::Extrinsic,
    ) -> TransactionValidity {
      Executive::validate_transaction(source, tx)
    }
  }

  impl sp_offchain::OffchainWorkerApi<Block> for Runtime {
    fn offchain_worker(header: &<Block as BlockT>::Header) {
      Executive::offchain_worker(header)
    }
  }

  impl sp_session::SessionKeys<Block> for Runtime {
    fn generate_session_keys(seed: Option<Vec<u8>>) -> Vec<u8> {
      SessionKeys::generate(seed)
    }

    fn decode_session_keys(
      encoded: Vec<u8>,
    ) -> Option<Vec<(Vec<u8>, KeyTypeId)>> {
      SessionKeys::decode_into_raw_public_keys(&encoded)
    }
  }

  impl frame_system_rpc_runtime_api::AccountNonceApi<Block, AccountId, Index> for Runtime {
    fn account_nonce(account: AccountId) -> Index {
      System::account_nonce(account)
    }
  }

  impl pallet_contracts_rpc_runtime_api::ContractsApi<Block, AccountId, Balance, BlockNumber>
    for Runtime
  {
    fn call(
      origin: AccountId,
      dest: AccountId,
      value: Balance,
      gas_limit: u64,
      input_data: Vec<u8>,
    ) -> pallet_contracts_primitives::ContractExecResult {
        Contracts::bare_call(origin, dest.into(), value, gas_limit, input_data)
    }

    fn get_storage(
      address: AccountId,
      key: [u8; 32],
    ) -> pallet_contracts_primitives::GetStorageResult {
      Contracts::get_storage(address, key)
    }

    fn rent_projection(
      address: AccountId,
    ) -> pallet_contracts_primitives::RentProjectionResult<BlockNumber> {
      Contracts::rent_projection(address)
    }
  }

  impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<Block, Balance> for Runtime {
    fn query_info(
      uxt: <Block as BlockT>::Extrinsic,
      len: u32,
    ) -> pallet_transaction_payment_rpc_runtime_api::RuntimeDispatchInfo<Balance> {
      TransactionPayment::query_info(uxt, len)
    }
    fn query_fee_details(uxt: <Block as BlockT>::Extrinsic, len: u32) -> FeeDetails<Balance> {
      TransactionPayment::query_fee_details(uxt, len)
    }
  }

  impl clover_rpc_runtime_api::CurrencyBalanceApi<Block, AccountId, CurrencyId, Balance> for Runtime {
    fn account_balance(account: AccountId, currency_id: Option<CurrencyId>) -> sp_std::vec::Vec<(CurrencyId, Balance)> {
      let mut balances = sp_std::vec::Vec::new();
      match currency_id {
        None => {
          for cid in CurrencyId::into_enum_iter() {
            balances.push((cid, Currencies::total_balance(cid, &account)));
          }
        },
        Some(cid) => balances.push((cid, Currencies::total_balance(cid, &account)))
      }
      balances
    }
  }

  impl clover_rpc_runtime_api::CurrencyPairApi<Block> for Runtime {
    fn currency_pair() -> sp_std::vec::Vec<(CurrencyId, CurrencyId)> {
       let pair = CloverDex::get_existing_currency_pairs().0;
       pair
    }
  }

  impl clover_rpc_runtime_api::CurrencyExchangeApi<Block, AccountId, CurrencyId, Balance, Rate, Share> for Runtime {
    fn target_amount_available(source: CurrencyId, target: CurrencyId, amount: Balance) -> (Balance, sp_std::vec::Vec<CurrencyId>) {
      let balance = CloverDex::get_target_amount_available(source, target, amount);
      balance
    }

    fn supply_amount_needed(source: CurrencyId, target: CurrencyId, amount: Balance) -> (Balance, sp_std::vec::Vec<CurrencyId>) {
      let balance = CloverDex::get_supply_amount_needed(source, target, amount);
      balance
    }

    fn get_liquidity(account: Option<AccountId>) -> sp_std::vec::Vec<(CurrencyId, CurrencyId, Balance, Balance, Balance, Balance, Balance)> {
      let result = CloverDex::get_liquidity(account);
      result
    }

    fn get_exchange_rate() -> Rate {
      let result = CloverDex::get_exchange_fee();
      result
    }

    fn to_add_liquidity(source: CurrencyId, target: CurrencyId, source_amount: Balance, target_amount: Balance) -> (Share, Share) {
      let result = CloverDex::to_add_liquidity(source, target, source_amount, target_amount);
      result
    }

    fn get_staking_info(account: AccountId, currency_first: CurrencyId, currency_second: CurrencyId) -> (Share, Balance) {
      let result = Incentives::get_account_info(&account, &currency_first, &currency_second);
      (result.shares, result.accumlated_rewards)
    }
  }

  impl clover_rpc_runtime_api::IncentivePoolApi<Block, AccountId, CurrencyId, Balance, Share> for Runtime {
    fn get_all_incentive_pools() -> sp_std::vec::Vec<(CurrencyId, CurrencyId, Share, Balance)> {
      Incentives::get_all_incentive_pools()
    }
  }

  impl fp_rpc::EthereumRuntimeRPCApi<Block> for Runtime {
    fn chain_id() -> u64 {
        <Runtime as clover_evm::Config>::ChainId::get()
    }

    fn account_basic(address: H160) -> EVMAccount {
        EVM::account_basic(&address)
    }

    fn gas_price() -> U256 {
        <Runtime as clover_evm::Config>::FeeCalculator::min_gas_price()
    }

    fn account_code_at(address: H160) -> Vec<u8> {
        EVM::account_codes(address)
    }

    fn author() -> H160 {
        <clover_ethereum::Module<Runtime>>::find_author()
    }

    fn storage_at(address: H160, index: U256) -> H256 {
        let mut tmp = [0u8; 32];
        index.to_big_endian(&mut tmp);
        EVM::account_storages(address, H256::from_slice(&tmp[..]))
    }

    fn call(
        from: H160,
        to: H160,
        data: Vec<u8>,
        value: U256,
        gas_limit: U256,
        gas_price: Option<U256>,
        nonce: Option<U256>,
        estimate: bool,
    ) -> Result<clover_evm::CallInfo, sp_runtime::DispatchError> {
        let config = if estimate {
            let mut config = <Runtime as clover_evm::Config>::config().clone();
            config.estimate = true;
            Some(config)
        } else {
            None
        };

        <Runtime as clover_evm::Config>::Runner::call(
            from,
            to,
            data,
            value,
            gas_limit.low_u32(),
            gas_price,
            nonce,
            config.as_ref().unwrap_or(<Runtime as clover_evm::Config>::config()),
        ).map_err(|err| err.into())
    }

    fn create(
        from: H160,
        data: Vec<u8>,
        value: U256,
        gas_limit: U256,
        gas_price: Option<U256>,
        nonce: Option<U256>,
        estimate: bool,
    ) -> Result<clover_evm::CreateInfo, sp_runtime::DispatchError> {
        let config = if estimate {
            let mut config = <Runtime as clover_evm::Config>::config().clone();
            config.estimate = true;
            Some(config)
        } else {
            None
        };

        <Runtime as clover_evm::Config>::Runner::create(
            from,
            data,
            value,
            gas_limit.low_u32(),
            gas_price,
            nonce,
            config.as_ref().unwrap_or(<Runtime as clover_evm::Config>::config()),
        ).map_err(|err| err.into())
    }

    fn current_transaction_statuses() -> Option<Vec<TransactionStatus>> {
        Ethereum::current_transaction_statuses()
    }

    fn current_block() -> Option<clover_ethereum::Block> {
        Ethereum::current_block()
    }

    fn current_receipts() -> Option<Vec<clover_ethereum::Receipt>> {
        Ethereum::current_receipts()
    }

    fn current_all() -> (
        Option<clover_ethereum::Block>,
        Option<Vec<clover_ethereum::Receipt>>,
        Option<Vec<TransactionStatus>>
    ) {
        (
            Ethereum::current_block(),
            Ethereum::current_receipts(),
            Ethereum::current_transaction_statuses()
        )
    }
  }
}

cumulus_pallet_parachain_system::register_validate_block!(Runtime, Executive);
