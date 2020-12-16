//! *Corundum* is a crate with an idiomatic persistent memory programming
//! interface and leverages Rust’s type system to statically avoid
//! most common persistent memory programming bugs. Corundum lets programmers
//! develop persistent data structures using familiar Rust constructs and have
//! confidence that they will be free of those bugs.
//! 
//! # Statically Prevented Bugs
//! |     Common Bugs     | Explanation  <img width=700/> | Approach |
//! |         ---         |              ---              |    ---   |
//! | Inter-Pool Pointers | A pointer in another pool which is unavailable | Type checking pools in persistent pointers. |
//! | P-to-V Pointers     | A persistent pointer pointing at volatile memory | Persistent pointers accept only [`PSafe`] types and volatile pointers are `!PSafe`. Only, [`VCell`] allows single-execution P-to-V pointers. |
//! | V-to-P Pointers     | A volatile pointer keeping a zero-referenced object alive | Only [`VWeak`] allows V-to-P pointers which is a weak reference and does not keep data alive. |
//! | Unlogged Updates    | An unrecoverable update to persistent data | Modifications are enforced to be inside atomic [`transaction`]s. | 
//! | Data Race           | Updating persistent data simultaneously in two threads | Mutable borrowing is limited to [`Mutex`] which uses a transaction-wide lock to provide both atomicity and isolation. |
//! | Locked Mutex        | A persistent mutex remains locked on powerfail | [`Mutex`] uses [`VCell`] which resets at restart. |
//! | Memory Leaks\*      | An allocated memory becomes unreachable | Persistent objects, except the root object, cannot cross transaction boundaries, and memory allocation is available only inside a transaction. Therefore, the allocation can survive only if there is a reference from the root object (or a decedent of it) to the data. <br>\* Cyclic references are not prevented in this version, which lead to a memory leak. |
//!
//! 
//! # Persistent Objects
//!
//! Persistent objects in Corundum are available through persistent pointers:
//! * [`Pbox`]: A pointer type for persistent memory allocation.
//! * [`Prc`]: A single-threaded reference-counting persistent pointer.
//! * [`Parc`]: A thread-safe reference-counting persistent pointer.
//! 
//! # Programming Model
//! Persistent memory is available as a file on a DAX-enable file system such as
//! EXT4-DAX or NOVA. These files are called memory pools. Corundum allows
//! memory pool types rather than memory pool objects to enforce pointer safety
//! while compilation. The trait [`MemPool`] provides the necessary
//! functionalities for the pool type.
//! 
//! The first step is to open a memory pool file in the program to be able to
//! work with persistent data. The [`default`] module provides a default memory
//! pool type ([`BuddyAlloc`]). To open a pool, we can invoke [`open<T>()`]
//! function which [initializes and] returns a reference to the root object of
//! type `T`. 
//! 
//! Data modification is provided and allowed only through [`transaction`]al
//! interface. None of the persistent pointers is mutably dereferencing for
//! safety. Mutable objects are allowed via interior mutability of any of the
//! following memory cells:
//! 
//! * [`LogCell<T,P>`] (or [`PCell<T>`]): An unborrowable, mutable persistent
//! memory location for a value of type `T` in pool `P`.
//! * [`LogRefCell<T,P>`] (or [`PRefCell<T>`]): A mutable persistent memory location with
//! dynamically checked borrow rules for a value of type `T` in pool `P`.
//! * [`Mutex<T,P>`] (or [`PMutex<T>`]): A mutual exclusion primitive useful for
//! protecting shared persistent data of type `T` in pool `P`.
//! 
//! The following example creates a pool file for a linked-list-based stack,
//! obtains a root object of type `Node`.
//! 
//! ```
//! use corundum::default::*;
//! 
//! // Aliasing the pool type for convenience
//! type P = BuddyAlloc;
//! 
//! #[derive(Root)]
//! struct Node {
//!     value: i32,
//!     next: PRefCell<Option<Prc<Node>>>
//! }
//! 
//! fn main() {
//!     let head = P::open::<Node>("foo.pool", O_CF).unwrap();
//! 
//!     P::transaction(|j| {
//!         let mut h = head.next.borrow_mut(j);
//!         *h = Some(Prc::new(Node {
//!             value: rand::random(),
//!             next: head.next.pclone(j)
//!         }, j));
//!     }).expect("Unsuccessful transaction");
//! }
//! ```
//! 
//! [`PSafe`]: ./trait.PSafe.html
//! [`VCell`]: ./cell/struct.VCell.html
//! [`VWeak`]: ./prc/struct.VWeak.html
//! [`transaction`]:  ./alloc/trait.MemPool.html#method.transaction
//! [`Mutex`]: ./sync/struct.Mutex.html
//! [`Pbox`]: ./boxed/struct.Pbox.html
//! [`Prc`]: ./prc/struct.Prc.html
//! [`Parc`]: ./sync/struct.Parc.html
//! [`MemPool`]: ./alloc/trait.MemPool.html
//! [`default`]: ./alloc/default/index.html
//! [`BuddyAlloc`]: ./alloc/default/struct.BuddyAlloc.html
//! [`LogCell<T,P>`]: ./cell/struct.LogCell.html
//! [`PCell<T>`]: ./alloc/default/type.PCell.html
//! [`LogRefCell<T,P>`]: ./cell/struct.LogRefCell.html 
//! [`PRefCell<T>`]: ./alloc/default/type.PRefCell.html
//! [`Mutex<T,P>`]: ./sync/struct.Mutex.html
//! [`PMutex<T>`]: ./alloc/default/type.PMutex.html
//! [`open<T>()`]: ./alloc/struct.MemPool.html#method.open

#![feature(auto_traits)]
#![feature(untagged_unions)]
#![feature(const_fn)]
#![feature(specialization)]
#![feature(concat_idents)]
#![feature(llvm_asm)]
#![feature(core_intrinsics)]
#![feature(thread_id_value)]
#![feature(negative_impls)]
#![feature(backtrace)]
#![feature(trusted_len)]
#![feature(exact_size_is_empty)]
#![feature(alloc_layout_extra)]
#![feature(dropck_eyepatch)]
#![feature(trivial_bounds)]
#![feature(stmt_expr_attributes)]
#![feature(trait_alias)]
#![feature(slice_concat_trait)]
#![feature(slice_partition_dedup)]
#![feature(const_generics)]
#![feature(type_name_of_val)]
#![feature(pattern)]
#![feature(try_reserve)]
#![feature(str_internals)]
#![feature(toowned_clone_into)]
#![feature(fn_traits)]
#![feature(unboxed_closures)]

#![allow(dead_code)]
#![allow(incomplete_features)]
#![allow(type_alias_bounds)]

#[macro_use]
extern crate lazy_static;
extern crate crndm_derive;
extern crate impl_trait_for_tuples;

pub mod alloc;
pub mod boxed;
pub mod cell;
pub mod clone;
pub mod ll;
pub mod prc;
pub mod sync;
pub mod ptr;
pub mod stm;
pub mod str;
pub mod vec;
pub mod convert;

mod utils;
mod marker;
mod tests;

pub use cell::RootObj;
pub use marker::*;

pub use alloc::default;
pub use crndm_derive::*;
pub use stm::transaction;

/// A `Result` type with string error messages
pub mod result {
    pub type Result<T: ?Sized> = std::result::Result<T, String>;
}

#[inline]
#[doc(hidden)]
pub(crate) fn as_mut<T>(v: &T) -> &mut T {
    unsafe {
        &mut *(v as *const T as *mut T)
    }
}