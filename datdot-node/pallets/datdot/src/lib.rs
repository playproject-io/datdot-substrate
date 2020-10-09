#![cfg_attr(not(feature = "std"), no_std)]

/******************************************************************************
  A runtime module template with necessary imports
******************************************************************************/

use sp_std::prelude::*;
use sp_std::fmt::Debug;
use sp_std::collections::btree_map::BTreeMap;
use sp_std::iter::ExactSizeIterator;
use sp_std::iter::Take;
use frame_support::{
	decl_module,
	decl_storage,
	decl_event,
	decl_error,
	debug::native,
	fail,
	ensure,
	Parameter,
	storage::{
		StorageMap,
		StorageValue,
		IterableStorageMap,
		IterableStorageDoubleMap,
	},
	traits::{
		EnsureOrigin,
		Get,
		Randomness,
		schedule::Named as ScheduleNamed,
		LockableCurrency
	},
	weights::{
		Pays,
		DispatchClass::{
			Operational,
		}
	},
};
use sp_std::convert::{
	TryInto,
};
use frame_system::{
	self as system,
	ensure_signed,
	ensure_root,
	RawOrigin
};
use codec::{
	Encode,
	Decode,
	Codec,
	EncodeLike
};
use sp_core::{
	ed25519,
	H256,
	H512,
};
use sp_runtime::{
	RuntimeDebug,
	traits::{
		Verify,
		CheckEqual,
		Dispatchable,
		SimpleBitOps,
		MaybeDisplay,
		TrailingZeroInput,
		AtLeast32Bit,
		MaybeSerializeDeserialize,
		Member,
		Scale
	},
};
use sp_arithmetic::{
	Percent,
	traits::{BaseArithmetic, One}
};
use rand_chacha::{rand_core::{RngCore, SeedableRng}, ChaChaRng};
use ed25519::{Public, Signature};

/******************************************************************************
  The module's configuration trait
******************************************************************************/
pub trait Trait: system::Trait{
	type Event: From<Event<Self>> + Into<<Self as system::Trait>::Event>;

	/// Type used for expressing timestamp.
	type Moment: Parameter + Default + AtLeast32Bit
		+ Scale<Self::BlockNumber, Output = Self::Moment> + Copy;

	
	/// The currency trait.
	type Currency: LockableCurrency<Self::AccountId>;

	type Hash:
	Parameter + Member + MaybeSerializeDeserialize + Debug + MaybeDisplay + SimpleBitOps
	+ Default + Copy + CheckEqual + sp_std::hash::Hash + AsRef<[u8]> + AsMut<[u8]>;
	type Randomness: Randomness<<Self as system::Trait>::Hash>;
	//ID TYPES---
	type FeedId: Parameter + Member + AtLeast32Bit + BaseArithmetic + EncodeLike<u32> + Codec + Default + Copy +
	MaybeSerializeDeserialize + Debug;
	type UserId: Parameter + Member + AtLeast32Bit + BaseArithmetic + EncodeLike<u32> + Codec + Default + Copy +
	MaybeSerializeDeserialize + Debug;
	type ContractId: Parameter + Member + AtLeast32Bit + BaseArithmetic + EncodeLike<u32> + Codec + Default + Copy +
	MaybeSerializeDeserialize + Debug;
	type ChallengeId: Parameter + Member + AtLeast32Bit + BaseArithmetic + EncodeLike<u32> + Codec + Default + Copy +
	MaybeSerializeDeserialize + Debug;
	type PlanId: Parameter + Member + AtLeast32Bit + BaseArithmetic + EncodeLike<u32> + Codec + Default + Copy +
	MaybeSerializeDeserialize + Debug;
	type PerformanceChallengeId: Parameter + Member + AtLeast32Bit + BaseArithmetic + EncodeLike<u32> + Codec + Default + Copy +
	MaybeSerializeDeserialize + Debug;
	// ---

	//CONSTS
	type PerformanceAttestorCount: Get<u8>;
}


/******************************************************************************
  Events
******************************************************************************/
decl_event!(
	pub enum Event<T> where
	<T as Trait>::FeedId,
	<T as Trait>::ContractId,
	<T as Trait>::PlanId,
	<T as Trait>::ChallengeId,
	<T as Trait>::PerformanceChallengeId
	{
		/// New data feed registered
		NewFeed(FeedId),
		/// New hosting plan by publisher for selected feed (many possible plans per feed)
		NewPlan(PlanId),
		/// A new contract between publisher, encoder, and hoster (many contracts per plan)
		/// (Encoder, Hoster,...)
		NewContract(ContractId),
		/// Hosting contract started
		HostingStarted(ContractId),
		/// New proof-of-storage challenge
		NewStorageChallenge(ChallengeId),
		/// Proof-of-storage confirmed
		ProofOfStorageConfirmed(ChallengeId),
		/// Proof-of-storage not confirmed
		ProofOfStorageFailed(ChallengeId),
		/// PerformanceChallenge of retrievability requested
		NewPerformanceChallenge(PerformanceChallengeId),
		/// Proof of retrievability confirmed
		AttestationReportConfirmed(PerformanceChallengeId),
		/// Data serving not verified
		AttestationReportFailed(PerformanceChallengeId),
	}
);

type RoleValue = Option<u32>;
type ChunkIndex = u64;

#[derive(Decode, PartialEq, Eq, Encode, Clone, RuntimeDebug)]
enum Role {
	Encoder,
	IdleEncoder,
	Hoster,
	IdleHoster,
	Attestor,
	IdleAttestor
}

type NoiseKey = Public;

#[derive(Decode, PartialEq, Eq, Encode, Clone, Default, RuntimeDebug)]
struct User<UserId, AccountId, Time> {
	id: UserId,
	address: AccountId,
	attestor_key: Option<NoiseKey>,
	encoder_key: Option<NoiseKey>,
	hoster_key: Option<NoiseKey>,
	attestor_form: Option<Form<Time>>,
	encoder_form: Option<Form<Time>>,
	hoster_form: Option<Form<Time>>
}

type FeedKey = Public;

#[derive(Decode, PartialEq, Eq, Encode, Clone, Default, RuntimeDebug)]
struct Feed<T: Trait> {
	id: T::FeedId,
	publickey: FeedKey,
	meta: TreeRoot,
	publisher: T::UserId
}

type Ranges<C> = Vec<(C, C)>;

#[derive(Decode, PartialEq, Eq, Encode, Clone, RuntimeDebug)]
pub struct ParentHashInRoot {
	hash: H256,
	hash_number: u64,
	total_length: u64
}

#[derive(Decode, PartialEq, Eq, Encode, Clone, Default, RuntimeDebug)]
pub struct TreeRoot {
	signature: H512,
	hash_type: u8, //2
	children: Vec<ParentHashInRoot>
}

#[derive(Decode, PartialEq, Eq, Encode, Clone, RuntimeDebug)]
pub struct TreeHashPayload {
	hash_type: u8, //2
	children: Vec<ParentHashInRoot>
}

#[derive(Decode, PartialEq, Eq, Encode, Clone, Default, RuntimeDebug)]
pub struct PlanUntil<Time> {
	time: Option<Time>,
	budget: Option<u64>,
	traffic: Option<u64>,
	price: Option<u64>
}

#[derive(Decode, PartialEq, Eq, Encode, Clone, Default, RuntimeDebug)]
pub struct Plan<PlanId, FeedId, Time, UserId> {
	id: PlanId,
	feeds: Vec<FeedId>,
	from: Time,
	until: PlanUntil<Time>,
	importance: u8,
	config: Config,
	schedules: Vec<Schedule<Time>>,
	sponsor: UserId,
	unhosted_feeds: Vec<FeedId>
}

struct Expirable<Item, Time> {
	inner: Item,
	expires: Time
}
#[derive(Decode, PartialEq, Eq, Encode, Clone, Default, RuntimeDebug)]
struct Contract<T: Trait> {
	id: T::ContractId,
	plan: T::PlanId,
	ranges: Ranges<ChunkIndex>,
	encoders: Vec<T::UserId>,
	hosters: Vec<T::UserId>,
	active: Vec<T::UserId>,
	attestor: T::UserId
}

#[derive(Decode, PartialEq, Eq, Encode, Clone, Default, RuntimeDebug)]
struct StorageChallenge<T: Trait> {
	id: T::ChallengeId,
	contract: T::ContractId,
	hoster: T::UserId,
	chunks: Vec<ChunkIndex>,
	attestor: Option<T::UserId>
}

#[derive(Decode, PartialEq, Eq, Encode, Clone, RuntimeDebug)]
pub struct Node {
	index: u64,
	hash: H256,
	size: u64
}

impl Node {
	fn height(&self) -> u64 {
		Self::get_height(self.index)
	}

	fn get_height(index : u64) -> u64 {
		let mut bit_pointer : u64 = 1;
		let mut cur_height : u64 = 0;
		while (index | bit_pointer) == index {
			cur_height += 1;
			bit_pointer *= 2;
		}
		cur_height
	}

	//get the index as if it were a leaf
	fn relative_index(&self) -> u64 {
		Self::index_at_height(self.index, self.height())
	}

	fn index_at_height(index : u64, height : u64) -> u64 {
		index / 2u64.pow(height.try_into().unwrap())
	}

	//get the last index at a given height, given a max leaf index
	fn top_index_at_height(height : u64, max_index : u64) -> Option<u64> {
		let offset = 2u64.pow(height.try_into().unwrap())-1;
		let interval = 2u64.pow((height+1).try_into().unwrap());
		let mut furthest_leaf = 0;
		let mut result = None;
		if height == 0 {
			result = Some(max_index);
		} else {
			for i in 0..height { //not inclusive of height intended
				furthest_leaf += 2u64.pow(i.try_into().unwrap());
			}
		}
		if max_index > offset {
			let mut next_result = true;
			while next_result {
				match result {
					Some(index) => {
						if index + interval + furthest_leaf > max_index {
							next_result = false;
						} else {
							result = Some(index+interval);
						}
					},
					None => {
						result = Some(offset);
					},
				}
			}
		}
		result
	}

	//get the highest height, given a max leaf index
	fn highest_at_index(max_index : u64) -> u64 {
		//max_index == 2^n - 2
		//return n
		//TODO: needs tests/audit, this was written using trail and error.
		let mut current_index = Some(max_index);
		let mut current_height : u64 = 0;
		while current_index.unwrap_or(0) > 2u64.pow(current_height.try_into().unwrap()) {
			current_index = current_index
				.unwrap_or(0)
				.checked_sub(2u64.pow((current_height+1).try_into().unwrap()));
			match current_index {
				Some(_) => current_height += 1,
				None => (),
			}
		}
		current_height
	}


	//get indexes of nodes in a merkle tree that are used to
	//calculate the root hash
	fn get_orphan_indeces(highest_index : u64) -> Vec<u64> {
		let mut indeces : Vec<u64> = Vec::new();
		for i in 0..Self::highest_at_index(highest_index)+1{
			match Self::top_index_at_height(i, highest_index) {
				Some(expr) => if expr % 2 == 0 {
					indeces.push(expr);
				},
				None => (),
			}
		}
		indeces
	}

	fn is_orphan(&self, highest_index : u64) -> bool {
		Self::get_orphan_indeces(highest_index)
			.contains(&self.index)
	}
}

// #[derive(Decode, PartialEq, Eq, Encode, Clone, RuntimeDebug)]
// pub struct Proof {
// 	index: u64,
// 	nodes: Vec<Node>,
// 	signature: Option<Signature>
// }

pub type Proof = Public;

#[derive(Decode, PartialEq, Eq, Encode, Clone, Default, RuntimeDebug)]
struct PerformanceChallenge<T: Trait> {
	id: T::PerformanceChallengeId,
	attestors: Option<Vec<T::UserId>>,
	contract: T::ContractId
}

#[derive(Decode, PartialEq, Eq, Encode, Clone, RuntimeDebug)]
pub struct Report {
	location: u8,
	latency: Option<u8>
}
#[derive(Decode, Default, PartialEq, Eq, Encode, Clone, RuntimeDebug)]
pub struct Performance {
	availability: Percent,
	bandwidth: (u64, Percent),
	latency: (u16, Percent)
}

pub type Region = Vec<u8>;

#[derive(Decode, Default, PartialEq, Eq, Encode, Clone, RuntimeDebug)]
pub struct Config {
	performance: Performance,
	regions: Vec<(Region, Performance)>
}

#[derive(Decode, PartialEq, Eq, Encode, Clone, RuntimeDebug)]
pub struct Schedule<Time> {
	duration: Time,
	delay: Time,
	interval: Time,
	repeat: u32,
	config: Config
}

#[derive(Decode, Default, PartialEq, Eq, Encode, Clone, RuntimeDebug)]
pub struct Form<Time> {
	storage: u64,
	idle_storage: u64,
	from: Time,
	until: Time,
	schedules: Vec<Schedule<Time>>,
	config: Config,
}

// (number of attestors required, job id)
#[derive(Decode, PartialEq, Eq, Encode, Clone, RuntimeDebug)]
pub enum AttestorJob<ChallengeId, PerformanceChallengeId> {
	StorageChallenge(ChallengeId),
	PerformanceChallenge(PerformanceChallengeId)
}

/******************************************************************************
  Storage items/db
******************************************************************************/
/* expected js storage queries
const queries = {
	getFeedByID,
	getFeedByKey,
	getUserByID,
	getPlanByID,
	getContractByID,
	getStorageChallengeByID,
	getPerformanceChallengeByID,
}
*/
decl_storage! {
	trait Store for Module<T: Trait> as DatVerify {
		// PUBLIC/API
		pub GetFeedByID: map hasher(twox_64_concat) T::FeedId => Option<Feed<T>>;
		pub GetFeedByKey: map hasher(twox_64_concat) FeedKey => Option<Feed<T>>;
        pub GetUserByID: map hasher(twox_64_concat) T::UserId => Option<User<T::UserId, T::AccountId, T::Moment>>;
        pub GetContractByID: map hasher(twox_64_concat) T::ContractId => Option<Contract<T>>;
        pub GetChallengeByID: map hasher(twox_64_concat) T::ChallengeId => Option<StorageChallenge<T>>;
		pub GetPlanByID: map hasher(twox_64_concat) T::PlanId => Option<Plan<T::PlanId, T::FeedId, T::Moment, T::UserId>>;
		pub GetPerformanceChallengeByID: map hasher(twox_64_concat) T::PerformanceChallengeId => Option<PerformanceChallenge<T>>;
		// INTERNALLY REQUIRED STORAGE
		pub GetNextFeedID: T::FeedId;
		pub GetNextUserID: T::UserId;
		pub GetNextContractID: T::ContractId;
		pub GetNextChallengeID: T::ChallengeId;
		pub GetNextPlanID: T::PlanId;
		pub GetNextAttestationID: T::PerformanceChallengeId;
		pub Nonce: u64;
		pub GetAttestorJobQueue: Vec<AttestorJob<T::ChallengeId, T::PerformanceChallengeId>>;
		// LOOKUPS (created as neccesary)
		pub GetUserByKey: map hasher(twox_64_concat) T::AccountId => Option<User<T::UserId, T::AccountId, T::Moment>>;
		// ROLES ARRAY
		pub Roles: double_map hasher(twox_64_concat) Role, hasher(twox_64_concat) T::UserId => bool;
	}
}

/******************************************************************************
  External functions (including on_finalize, on_initialize)
******************************************************************************/
decl_module!{
	pub struct Module<T: Trait> for enum Call where origin: T::Origin {
		fn deposit_event() = default;

		/*
		if (type === 'publishFeed') _publishFeed(user, { name, nonce }, status, args)
		else if (type === 'publishPlan') _publishPlan(user, { name, nonce }, status, args)
		else if (type === 'registerEncoder') _registerEncoder(user, { name, nonce }, status, args)
		else if (type === 'registerAttestor') _registerAttestor(user, { name, nonce }, status, args)
		else if (type === 'registerHoster') _registerHoster(user, { name, nonce }, status, args)
		else if (type === 'encodingDone') _encodingDone(user, { name, nonce }, status, args)
		else if (type === 'hostingStarts') _hostingStarts(user, { name, nonce }, status, args)
		else if (type === 'requestStorageChallenge') _requestStorageChallenge(user, { name, nonce }, status, args)
		else if (type === 'requestPerformanceChallenge') _requestPerformanceChallenge(user, { name, nonce }, status, args)
		else if (type === 'submitStorageChallenge') _submitStorageChallenge(user, { name, nonce }, status, args)
		else if (type === 'submitPerformanceChallenge') _submitPerformanceChallenge(user, { name, nonce }, status, args)
		// else if ... 
		*/
		
		/*
  		const [merkleRoot]  = args
  		const [key, {hashType, children}, signature] = merkleRoot
  		const keyBuf = Buffer.from(key, 'hex')
  		// check if feed already exists
  		if (DB.feedByKey[keyBuf.toString('hex')]) return
  		const feed = { publickey: keyBuf.toString('hex'), meta: { signature, hashType, children } }
  		const feedID = DB.feeds.push(feed)
  		feed.id = feedID
  		// push to feedByKey lookup array
  		DB.feedByKey[keyBuf.toString('hex')] = feedID
  		const userID = user.id
  		feed.publisher = userID
  		// Emit event
  		const NewFeed = { event: { data: [feedID], method: 'FeedPublished' } }
  		const event = [NewFeed]
  		handlers.forEach(([name, handler]) => handler(event))
		*/
		#[weight = (100000, Operational, Pays::No)] //todo weight
		fn publish_feed(origin, merkle_root: (Public, TreeHashPayload, H512)){
			let publisher_address = ensure_signed(origin)?;
			let user = Self::load_user(publisher_address);
			if let Some(feed) = <GetFeedByKey<T>>::get(merkle_root.0){
				// if a feed is already published, emit error here.
			} else {
				let feed_id : T::FeedId;
				let next_feed_id = <GetNextFeedID<T>>::get();
				let new_feed = Feed::<T> {
					id: next_feed_id.clone(),
					publickey: merkle_root.0,
					meta: TreeRoot {
						signature: merkle_root.2,
						hash_type: merkle_root.1.hash_type,
						children: merkle_root.1.children
					},
					publisher: user.id
				};
				<GetFeedByID<T>>::insert(next_feed_id, new_feed.clone());
				<GetFeedByKey<T>>::insert(merkle_root.0, new_feed.clone());
				feed_id = next_feed_id.clone();
				<GetNextFeedID<T>>::put(next_feed_id+One::one());
				Self::deposit_event(RawEvent::NewFeed(feed_id));
			}
		}

		/*
  		log({ type: 'chain', body: [`Publishing a plan`] })
  		const [plan] = args
  		const { feeds, from, until, importance, config, schedules } =  plan
  		const userID = user.id
		plan.sponsor = userID // or patron?
		const planID = DB.plans.push(plan)
		plan.id = planID
		// Add planID to unhostedPlans
		DB.unhostedPlans.push(planID)
		// Add feeds to unhosted
		plan.unhostedFeeds = feeds
		// Find hosters,encoders and attestors
		tryContract({ plan, log })
		// Emit event
		const NewPlan = { event: { data: [planID], method: 'NewPlan' } }
		const event = [NewPlan]
		handlers.forEach(([name, handler]) => handler(event))
		*/
		#[weight = (100000, Operational, Pays::No)] //todo weight
		fn publish_plan(origin, plan: Plan<T::PlanId, T::FeedId, T::Moment, T::UserId>){
			let sponsor_address = ensure_signed(origin)?;
			let user = Self::load_user(sponsor_address);
			let next_plan_id = <GetNextPlanID<T>>::get();
			let new_plan = Plan::<T::PlanId, T::FeedId, T::Moment, T::UserId> {
				id: next_plan_id.clone(),
				sponsor: user.id,
				..plan
			};
			<GetPlanByID<T>>::insert(next_plan_id, new_plan.clone());
			let plan_id = next_plan_id.clone();
			<GetNextPlanID<T>>::put(next_plan_id+One::one());
			Self::deposit_event(RawEvent::NewPlan(plan_id.clone()));
			
		}

		/*
		const log = connections[name].log
		const userID = user.id
		const [encoderKey, form] = args
		if (DB.users[userID-1].encoderKey) return log({ type: 'chain', body: [`User is already registered as encoder`] })
		const keyBuf = Buffer.from(encoderKey, 'hex')
		DB.users[userID - 1].encoderKey = keyBuf.toString('hex')
		DB.users[userID - 1].encoderForm = form
		DB.idleEncoders.push(userID)
		tryContract({ log })

		*/
		#[weight = (100000, Operational, Pays::No)] //todo weight
		fn register_encoder(origin, encoder_key: NoiseKey, form: Form<T::Moment>){

			let user_address = ensure_signed(origin)?;
			let mut user = Self::load_user(user_address);
			user.encoder_form = Some(form);
			user.encoder_key = Some(encoder_key);
			Self::save_user(&mut user);
			<Roles<T>>::insert(Role::Encoder, user.id, true);
			<Roles<T>>::insert(Role::IdleEncoder, user.id, true);
		}

		/*
		const log = connections[name].log
		const userID = user.id
		const [hosterKey, form] = args
		// @TODO emit event or make a callback to notify the user
		if (DB.users[userID-1].hosterKey) return log({ type: 'chain', body: [`User is already registered as a hoster`] })
		const keyBuf = Buffer.from(hosterKey, 'hex')
		DB.users[userID - 1].hosterKey = keyBuf.toString('hex')
		DB.users[userID - 1].hosterForm = form
		DB.idleHosters.push(userID)
		tryContract({ log })
		*/
		#[weight = (100000, Operational, Pays::No)] //todo weight
		fn register_hoster(origin, hoster_key: NoiseKey, form: Form<T::Moment>){
			let user_address = ensure_signed(origin)?;
			let mut user = Self::load_user(user_address);
			user.hoster_form = Some(form);
			user.hoster_key = Some(hoster_key);
			Self::save_user(&mut user);
			<Roles<T>>::insert(Role::Hoster, user.id, true);
			<Roles<T>>::insert(Role::IdleHoster, user.id, true);
		}

		/*
		const userID = user.id
		const [attestorKey, form] = args
		if (DB.users[userID-1].attestorKey) return log({ type: 'chain', body: [`User is already registered as a attestor`] })
		const keyBuf = Buffer.from(attestorKey, 'hex')
		DB.users[userID - 1].attestorKey = keyBuf.toString('hex')
		DB.users[userID - 1].attestorForm = form
		DB.idleAttestors.push(userID)
		checkAttestorJobs(log)
		tryContract({ log })
		*/
		#[weight = (100000, Operational, Pays::No)] //todo weight
		fn register_attestor(origin, attestor_key: NoiseKey, form: Form<T::Moment>){
			let user_address = ensure_signed(origin)?;
			let mut user = Self::load_user(user_address);
			user.attestor_form = Some(form);
			user.attestor_key = Some(attestor_key);
			Self::save_user(&mut user);
			<Roles<T>>::insert(Role::Attestor, user.id, true);
			Self::give_attestors_jobs(&mut [user.id], &mut []);
		}

		// Ensure that these match new logic TODO
		#[weight = (100000, Operational, Pays::No)] //todo weight
		fn unregister_encoder(origin){
			
			let user_address = ensure_signed(origin)?;
			let mut user = Self::load_user(user_address);
			user.encoder_form = None;
			user.encoder_key = None;
			Self::save_user(&mut user);
			<Roles<T>>::remove(Role::Encoder, user.id);
			<Roles<T>>::remove(Role::IdleEncoder, user.id);
		}

		// Ensure that these match new logic TODO
		#[weight = (100000, Operational, Pays::No)] //todo weight
		fn unregister_hoster(origin){
			let user_address = ensure_signed(origin)?;
			let mut user = Self::load_user(user_address);
			user.hoster_form = None;
			user.hoster_key = None;
			Self::save_user(&mut user);
			<Roles<T>>::remove(Role::Hoster, user.id);
			<Roles<T>>::remove(Role::IdleHoster, user.id);

		}

		// Ensure that these match new logic TODO
		#[weight = (100000, Operational, Pays::No)] //todo weight
		fn unregister_attestor(origin){	
			let user_address = ensure_signed(origin)?;
			let mut user = Self::load_user(user_address);
			user.attestor_form = None;
			user.attestor_key = None;
			Self::save_user(&mut user);
			<Roles<T>>::remove(Role::Attestor, user.id);
			<Roles<T>>::remove(Role::IdleAttestor, user.id);
		}

		/*
		// @TODO check if encodingDone and only then trigger hostingStarts
		const [ contractID ] = args
		DB.contractsHosted.push(contractID)
		const contract = DB.contracts[contractID - 1]
		// if hosting starts, also the attestor finished job, add them to idleAttestors again
		const attestorID = contract.attestor
		if (!DB.idleAttestors.includes(attestorID)) {
			DB.idleAttestors.push(attestorID)
			checkAttestorJobs(log)
		}
		const userID = user.id
		const confirmation = { event: { data: [contractID, userID], method: 'HostingStarted' } }
		const event = [confirmation]
		handlers.forEach(([name, handler]) => handler(event))
		// log({ type: 'chain', body: [`emit chain event ${JSON.stringify(event)}`] })
		*/
		#[weight = (100000, Operational, Pays::No)] //todo weight
		fn hosting_starts(origin, contract_id: T::ContractId ){
			let user_address = ensure_signed(origin)?;
			let user = Self::load_user(user_address);
			if let Some(mut contract) = <GetContractByID<T>>::get(contract_id){
				ensure!(contract.hosters.contains(&user.id), "TODO permission error");
				if !contract.active.contains(&user.id) {
					contract.active.push(user.id);
				} else if contract.active.len() == contract.hosters.len() {
					for current_encoder in contract.clone().encoders {
						<Roles<T>>::insert(Role::IdleEncoder, current_encoder.clone(), true);
					}
				}
				<GetContractByID<T>>::insert(contract_id, contract.clone());
				Self::give_attestors_jobs(&[contract.attestor], &mut []);
				Self::deposit_event(RawEvent::HostingStarted(contract_id));
			};
			//todo should be economic logic, still under consideration.
		}

		/*

		const [ contractID, hosterID ] = args
		const ranges = DB.contracts[contractID - 1].ranges // [ [0, 3], [5, 7] ]
		// @TODO currently we check one random chunk in each range => find better logic
		const chunks = ranges.map(range => getRandomInt(range[0], range[1] + 1))
		const storageChallenge = { contract: contractID, hoster: hosterID, chunks }
		const storageChallengeID = DB.storageChallenges.push(storageChallenge)
		storageChallenge.id = storageChallengeID
		const attestorID = getAttestor(storageChallenge, log)
		if (!attestorID) return
		storageChallenge.attestor = attestorID
		// emit events
		const challenge = { event: { data: [storageChallengeID], method: 'NewStorageChallenge' } }
		const event = [challenge]
		handlers.forEach(([name, handler]) => handler(event))

		*/
		#[weight = (100000, Operational, Pays::No)] //todo weight
		fn request_storage_challenge(origin, contract_id: T::ContractId, hoster_id: T::UserId ){
			let user_address = ensure_signed(origin)?;
			let user = Self::load_user(user_address);
			if let Some(contract) = <GetContractByID<T>>::get(&contract_id){
				let sponsor = <GetPlanByID<T>>::get(contract.plan).ok_or("TODO no plan")?.sponsor;
				ensure!(sponsor == user.id, "TODO invalid challenge request");
				let ranges = contract.ranges;
				let random_chunks = Self::random_from_ranges(ranges);
				let challenge_id = <GetNextChallengeID<T>>::get();
				let challenge = StorageChallenge::<T> {
					id: challenge_id.clone(),
					contract: contract_id,
					hoster: hoster_id,
					chunks: random_chunks,
					attestor: None
				};
				<GetChallengeByID<T>>::insert(challenge_id, challenge.clone());
				<GetNextChallengeID<T>>::put(challenge_id.clone()+One::one());
				Self::give_attestors_jobs(&[], &mut [AttestorJob::StorageChallenge(challenge_id.clone())]);
			} else {
				//some err
			}
		}

		/*
		const [ storageChallengeID, proofs ] = args
		const storageChallenge = DB.storageChallenges[storageChallengeID - 1]
		// attestor finished job, add them to idleAttestors again
		const attestorID = storageChallenge.attestor
		if (!DB.idleAttestors.includes(attestorID)) {
		DB.idleAttestors.push(attestorID)
		checkAttestorJobs(log)
		}
		// @TODO validate proof
		const isValid = validateProof(proofs, storageChallenge)
		let proofValidation
		const data = [storageChallengeID]
		log({ type: 'chain', body: [`StorageChallenge Proof for challenge: ${storageChallengeID}`] })
		if (isValid) response = { event: { data, method: 'StorageChallengeConfirmed' } }
		else response = { event: { data: [storageChallengeID], method: 'StorageChallengeFailed' } }
		// emit events
		const event = [response]
		handlers.forEach(([name, handler]) => handler(event))
		// log({ type: 'chain', body: [`emit chain event ${JSON.stringify(event)}`] })
		*/
		#[weight = (100000, Operational, Pays::No)] //todo weight
		fn submit_storage_challenge(origin, challenge_id: T::ChallengeId, proofs: Vec<Proof> ){
			let user_address = ensure_signed(origin)?;
			let mut success: bool = true;
			if let Some(challenge) = <GetChallengeByID<T>>::get(&challenge_id){
				for proof in proofs {
					if Self::validate_proof(proof.clone(), challenge.clone()){
						success = success && true;
					} else {
						success = success && false;
					}
				}
				if success {
					Self::deposit_event(RawEvent::ProofOfStorageConfirmed(challenge_id.clone()));
				} else {
					Self::deposit_event(RawEvent::ProofOfStorageFailed(challenge_id.clone()));
				}
			} else {
				// TODO invalid challenge
			}
		}

		/*
		const log = connections[name].log
		const [ contractID ] = args
		const performanceChallenge = { contract: contractID }
		const performanceChallengeID = DB.performanceChallenges.push(performanceChallenge)
		performanceChallenge.id = performanceChallengeID
		if (DB.idleAttestors.length >= 5) emitPerformanceChallenge(performanceChallenge, log)
		else DB.attestorJobs.push({ fnName: 'emitPerformanceChallenge', opts: performanceChallenge })
		*/
		#[weight = (100000, Operational, Pays::No)] //todo weight
		fn request_performance_challenge(origin, contract_id: T::ContractId ){
			let user_address = ensure_signed(origin)?;
			let attestation_id = <GetNextAttestationID<T>>::get();
			let attestation = PerformanceChallenge::<T> {
				id: attestation_id.clone(),
				attestors: None,
				contract: contract_id
			};
			<GetPerformanceChallengeByID<T>>::insert(attestation_id, attestation.clone());
			Self::give_attestors_jobs(&mut [], &mut [AttestorJob::PerformanceChallenge(attestation_id)]);
		}

		/*
		const [ performanceChallengeID, report ] = args
		const performanceChallenge = DB.performanceChallenges[performanceChallengeID - 1]
		// attestor finished job, add them to idleAttestors again
		const attestorID = user.id
		if (!DB.idleAttestors.includes(attestorID)) {
			DB.idleAttestors.push(attestorID)
			checkAttestorJobs(log)
		}
		// emit events
		if (report) response = { event: { data: [performanceChallengeID], method: 'PerformanceChallengeConfirmed' } }
		else response = { event: { data: [performanceChallengeID], method: 'PerformanceChallengeFailed' } }
		const event = [response]
		handlers.forEach(([name, handler]) => handler(event))
		*/
		#[weight = (100000, Operational, Pays::No)] //todo weight
		fn submit_performance_challenge(origin, attestation_id: T::PerformanceChallengeId, reports: Vec<Report>){
			let user_address = ensure_signed(origin)?;
			let mut success: bool = true;
			if let Some(attestation) = <GetPerformanceChallengeByID<T>>::get(&attestation_id){
				for report in reports {
					match report.latency {
						Some(_) => {
							// report passed
							success = success && true;
						},
						_ => {
							// report failed
							success = success && false;
						}
					}
				}
				if success {
					Self::deposit_event(RawEvent::AttestationReportConfirmed(attestation_id.clone()));
				} else {
					Self::deposit_event(RawEvent::AttestationReportFailed(attestation_id.clone()));
				}
			}
		}
	}
}

/******************************************************************************
  Internal functions
******************************************************************************/
impl<T: Trait> Module<T> {

	fn load_user(account_id: T::AccountId) -> User<T::UserId, T::AccountId, T::Moment> {
			if let Some(user) = <GetUserByKey<T>>::get(&account_id){
				user
			} else {
				let mut user_id = <GetNextUserID<T>>::get();
				let mut new_user = User::<T::UserId, T::AccountId, T::Moment> {
					id: user_id.clone(),
					address: account_id.clone(),
					..User::<T::UserId, T::AccountId, T::Moment>::default()
				};
				Self::save_user(&mut new_user);
				<GetNextUserID<T>>::put(user_id+One::one());
				new_user
			}
	}

	fn save_user(user: &mut User<T::UserId, T::AccountId, T::Moment>){
		<GetUserByID<T>>::insert(user.id, user.clone());
		<GetUserByKey<T>>::insert(user.clone().address, user.clone());
	}

	fn give_attestors_jobs(
		attestors: &[T::UserId],
		jobs: &mut [AttestorJob<T::ChallengeId, T::PerformanceChallengeId>]
	){
		for attestor in attestors {
			<Roles<T>>::insert(Role::IdleAttestor, attestor, true);
		}
		let mut old_job_queue = <GetAttestorJobQueue<T>>::get();
		let mut job_queue_slice = [jobs, old_job_queue.as_mut_slice()].concat();
		let job_queue = job_queue_slice.iter();
		for job in job_queue.clone() {
			match job {
				AttestorJob::PerformanceChallenge(challenge_id) => {
					// Verify we have a sufficient number of attestors for this challenge type
					let mut attestor_count = 0;
					let mut performance_attestors : Vec<T::UserId> = Vec::new();
						// emit an attestation request for each performance attestor
					for attestor in <Roles<T>>::iter_prefix(Role::IdleAttestor) {
						attestor_count += 1;
						performance_attestors.push(attestor.0);
						if attestor_count > T::PerformanceAttestorCount::get().into(){
							<Roles<T>>::remove(Role::IdleAttestor, attestor.0);
							if let Some(mut challenge) = <GetPerformanceChallengeByID<T>>::get(&challenge_id){
								challenge.attestors = Some(performance_attestors);
								<GetPerformanceChallengeByID<T>>::insert(&challenge_id, challenge);
							}
							Self::deposit_event(RawEvent::NewPerformanceChallenge(challenge_id.clone()));
							break;
						}
					}
				},
				AttestorJob::StorageChallenge(challenge_id) => {

				}
			}
		}
		<GetAttestorJobQueue<T>>::put(
			job_queue.collect::<Vec<&AttestorJob<T::ChallengeId, T::PerformanceChallengeId>>>()
		);
	}

	fn random_from_ranges(ranges: Ranges<ChunkIndex>) -> Vec<ChunkIndex>{
		//TODO, currently only returns first chunk of every available range,
		//should return some random selection.
		ranges.iter().map(|x|x.0).collect()
	}

	fn validate_proof(proof: Proof, challenge: StorageChallenge<T>) -> bool {
		//TODO validate proof!
		true
	}

	//borrowing from society pallet ---
	fn pick_usize<'a, R: RngCore>(rng: &mut R, max: usize) -> usize {

		(rng.next_u32() % (max as u32 + 1)) as usize
	}


	/// Pick an item at pseudo-random from the slice, given the `rng`. `None` iff the slice is empty.
	fn pick_item<'a, R: RngCore, E>(rng: &mut R, items: &'a [E]) -> Option<&'a E> {
		if items.is_empty() {
			None
		} else {
			Some(&items[Self::pick_usize(rng, items.len() - 1)])
		}
	}
	// ---

	fn unique_nonce() -> u64 {
		let nonce : u64 = <Nonce>::get();
		<Nonce>::put(nonce+1);
		nonce
	}

	fn get_random_of_vec<Item: Copy>(influence: &[u8], members: Vec<Item>, count: u32) -> Vec<Item>{
		let match_count : usize = count.try_into().unwrap();
		if members.len() <= match_count { members } else {
			let nonce : u64 = Self::unique_nonce();
			let mut random_select: Vec<Item> = Vec::new();
				// added indeces to seed in order to ensure challenges Get unique randomness.
			let seed = (nonce, T::Randomness::random(influence))
				.using_encoded(|b| <[u8; 32]>::decode(&mut TrailingZeroInput::new(b)))
				.expect("input is padded with zeroes; qed");
			let mut rng = ChaChaRng::from_seed(seed);
			let pick_item = |_| Self::pick_item(&mut rng, &members[..]).expect("exited if members empty; qed");
			for item in (0..count).map(pick_item){
				random_select.push(*item);
			}
			random_select
		}
	}

	fn get_random_of_role(influence: &[u8], role: &Role, count: u32) -> Vec<T::UserId> {
		let members : Vec<T::UserId> = <Roles<T>>::iter_prefix(role).filter_map(|x|{
			if x.1{
				Some(x.0)
			} else {
				None
			}
		}).collect();
		Self::get_random_of_vec(influence, members, count)
	}
}
