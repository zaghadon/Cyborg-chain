//! Pallet Edge Connect
//!
//! - [`Config`]
//! - [`Call`]
//! - [`Pallet`]
//! - [`Error`]
//! - [`Event`]
//! - [`Storage`]
//!
//! ## Overview
//!
//! Edge Connect is a pallet that allows users to create and remove connections between Cyborg
//! blockchain and external edge servers.
//!
//! ## Interface
//!
//! ### Dispatchable Functions
//!
//! * `create_connection` - Creates a connection between Cyborg blockchain and an external edge
//!   server.
//! * `send_command` - Sends a command to CyberHub.
//! * `receive_response` - Receives a response from CyberHub.
//! * `remove_connection` - Removes a connection between Cyborg blockchain and an external edge
//!   server.

#![cfg_attr(not(feature = "std"), no_std)]

use frame_system::{
	self as system,
	offchain::{
		AppCrypto, CreateSignedTransaction, SendSignedTransaction, SendUnsignedTransaction,
		SignedPayload, Signer, SigningTypes, SubmitTransaction,
	},
};
use sp_core::crypto::KeyTypeId;
use scale_info::prelude::string::String;


// #[cfg(test)]
// mod mock;

// #[cfg(test)]
// mod tests;

// #[cfg(feature = "runtime-benchmarks")]
// mod benchmarking;

pub const KEY_TYPE: KeyTypeId = KeyTypeId(*b"edge");

pub mod crypto {
	use super::KEY_TYPE;
	use sp_core::sr25519::Signature as Sr25519Signature;
	use sp_runtime::{
		app_crypto::{app_crypto, sr25519},
		traits::Verify,
		MultiSignature, MultiSigner,
	};
	app_crypto!(sr25519, KEY_TYPE);

	pub struct TestAuthId;

	impl frame_system::offchain::AppCrypto<MultiSigner, MultiSignature> for TestAuthId {
		type RuntimeAppPublic = Public;
		type GenericSignature = sp_core::sr25519::Signature;
		type GenericPublic = sp_core::sr25519::Public;
	}

	// implemented for mock runtime in test
	impl frame_system::offchain::AppCrypto<<Sr25519Signature as Verify>::Signer, Sr25519Signature>
		for TestAuthId
	{
		type RuntimeAppPublic = Public;
		type GenericSignature = sp_core::sr25519::Signature;
		type GenericPublic = sp_core::sr25519::Public;
	}
}

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: CreateSignedTransaction<Call<Self>> + frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// Authority ID used for offchain worker
		type AuthorityId: AppCrypto<Self::Public, Self::Signature>;
	}

	#[pallet::validate_unsigned]
	impl<T: Config> ValidateUnsigned for Pallet<T> {
		type Call = Call<T>;

		/// Validate unsigned call to this module.
		///
		/// By default unsigned transactions are disallowed, but implementing the validator
		/// here we make sure that some particular calls (the ones produced by offchain worker)
		/// are being whitelisted and marked as valid.
		fn validate_unsigned(source: TransactionSource, call: &Self::Call) -> TransactionValidity {
			let valid_tx = |provide| {
				ValidTransaction::with_tag_prefix("my-pallet")
					.priority(UNSIGNED_TXS_PRIORITY)
					.and_provides([&provide])
					.longevity(3)
					.propagate(true)
					.build()
			};

			match call {
				RuntimeCall::my_unsigned_tx { key: value } => valid_tx(b"my_unsigned_tx".to_vec()),
				_ => InvalidTransaction::Call.into(),
			}
		}
	}

	// The pallet's runtime storage items.
	#[pallet::storage]
	#[pallet::getter(fn connection)]
	pub type Connection<T> = StorageValue<_, u32>; // TODO: change to the proper data structure

	// Pallets use events to inform users when important changes are made.
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Event documentation should end with an array that provides descriptive names for event
		/// parameters. [connection, who]
		ConnectionCreated { connection: u32, who: T::AccountId },
		/// Event documentation should end with an array that provides descriptive names for event
		/// parameters. [connection, who]
		ConnectionRemoved { connection: u32, who: T::AccountId },
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// Returned if the connection already exists.
		ConnectionAlreadyExists,
		/// Returned if the connection does not exist.
		ConnectionDoesNotExist,
	}

	// The pallet's hooks for offchain worker
	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn offchain_worker(block_number: T::BlockNumber) {
			log::info!("Hello from offchain workers!");

			let signer = Signer::<T, T::AuthorityId>::all_accounts();
			if !signer.can_sign() {
				log::error!("No local accounts available");
				return
			}

			// Import `frame_system` and retrieve a block hash of the parent block.
			let parent_hash = <system::Pallet<T>>::block_hash(block_number - 1u32.into());
			log::debug!("Current block: {:?} (parent hash: {:?})", block_number, parent_hash);

			let response: Option<u32> = Self::receive_response(); // TODO: create receive_response function
			log::debug!("Response: {:?}", response);

			// This will send both signed and unsigned transactions
			// depending on the block number.
			// Usually it's enough to choose one or the other.
			let should_send = Self::choose_transaction_type(block_number);
			let res = match should_send {
				TransactionType::Signed => Self::fetch_response_and_send_signed(),
				TransactionType::UnsignedForAny =>
					Self::fetch_response_and_send_unsigned_for_any_account(block_number),
				TransactionType::UnsignedForAll =>
					Self::fetch_response_and_send_unsigned_for_all_accounts(block_number),
				TransactionType::Raw => Self::fetch_response_and_send_raw_unsigned(block_number),
				TransactionType::None => Ok(()),
			};
			if let Err(e) = res {
				log::error!("Error: {}", e);
			}
		}
	}

	// Public part of the pallet.
	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Create connection
		#[pallet::call_index(0)]
		#[pallet::weight(10_000 + T::DbWeight::get().writes(1).ref_time())]
		pub fn create_connection(origin: OriginFor<T>, connection: u32) -> DispatchResult {
			// Check that the extrinsic was signed and get the signer.
			let who = ensure_signed(origin)?;

			// Check that the connection does not already exist.
			ensure!(!<Connection<T>>::exists(), Error::<T>::ConnectionAlreadyExists);

			// Update storage.
			<Connection<T>>::put(connection);

			// Emit an event.
			Self::deposit_event(Event::ConnectionCreated { connection, who });

			// Return a successful DispatchResult
			Ok(())
		}

		// TODO:
		// Create functions for:
		// 1. send command (ocw)
		#[pallet::call_index(1)]
		#[pallet::weight({0})]
		pub fn send_command(origin: OriginFor<T>, command: String) -> DispatchResult {
			// Retrieve the signer and check it is valid.
			let who = ensure_signed(origin)?;

			// Check that the connection exists.
			ensure!(<Connection<T>>::exists(), Error::<T>::ConnectionDoesNotExist);

			// TODO: send command to ocw

			// Return a successful DispatchResult
			Ok(());
		}
		// 2. receive_response (ocw)
		#[pallet::call_index(2)]
		#[pallet::weight({0})]
		pub fn receive_response(origin: OriginFor<T>) -> DispatchResult {
			// Retrieve the signer and check it is valid.
			let who = ensure_signed(origin)?;

			// Check that the connection exists.
			ensure!(<Connection<T>>::exists(), Error::<T>::ConnectionDoesNotExist);

			// TODO: receive response from ocw

			// Return a successful DispatchResult
			Ok(());

			// 3. remove_connection
			#[pallet::call_index(3)]
			#[pallet::weight(10_000 + T::DbWeight::get().writes(1).ref_time())]
			pub fn remove_connection(origin: OriginFor<T>, connection: u32) -> DispatchResult {
				// Check that the extrinsic was signed and get the signer.
				let who = ensure_signed(origin)?;

				// Check that the connection exists.
				ensure!(<Connection<T>>::exists(), Error::<T>::ConnectionDoesNotExist);

				// Update storage.
				<Connection<T>>::kill();

				// Emit an event.
				Self::deposit_event(Event::ConnectionRemoved { connection, who });

				// Return a successful DispatchResult
				Ok(());
			}
		}
	}
}
