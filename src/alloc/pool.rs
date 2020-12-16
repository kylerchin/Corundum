use crate::cell::{RootCell, RootObj};
use crate::result::Result;
use crate::stm::{journal::*, Chaperon, Log};
use crate::{as_mut, PSafe, TxInSafe, TxOutSafe};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::ops::Range;
use std::panic::UnwindSafe;
use std::path::Path;
use std::thread::ThreadId;
use std::{alloc::Layout, mem, ptr};

/// Default pool memory size to be used while creating a new pool
pub const DEFAULT_POOL_SIZE: u64 = 8 * 1024 * 1024;

/// Open pool flags
pub mod open_flags {
    /// Open Flag: Create the pool memory file
    pub const O_C: u32 = 0x00000001;

    /// Open Flag: Formats the pool memory file if file exists, otherwise error
    pub const O_F: u32 = 0x00000002;

    /// Open Flag: Creates pool memory file only if it does not exist
    pub const O_CNE: u32 = 0x00000004;

    /// Open Flag: Creates and formats a new file
    pub const O_CF: u32 = O_C | O_F;

    /// Open Flag: Creates and formats pool memory file only if it does not exist
    pub const O_CFNE: u32 = O_CNE | O_F;

    /// Open Flag: Creates a pool memory file of size 1GB
    pub const O_1GB: u32 = 0x00000010;

    /// Open Flag: Creates a pool memory file of size 2GB
    pub const O_2GB: u32 = 0x00000020;

    /// Open Flag: Creates a pool memory file of size 4GB
    pub const O_4GB: u32 = 0x00000040;

    /// Open Flag: Creates a pool memory file of size 8GB
    pub const O_8GB: u32 = 0x00000080;

    /// Open Flag: Creates a pool memory file of size 16GB
    pub const O_16GB: u32 = 0x00000100;

    /// Open Flag: Creates a pool memory file of size 32GB
    pub const O_32GB: u32 = 0x00000200;

    /// Open Flag: Creates a pool memory file of size 64GB
    pub const O_64GB: u32 = 0x00000400;

    /// Open Flag: Creates a pool memory file of size 128GB
    pub const O_128GB: u32 = 0x00000800;

    /// Open Flag: Creates a pool memory file of size 256GB
    pub const O_256GB: u32 = 0x00001000;

    /// Open Flag: Creates a pool memory file of size 512GB
    pub const O_512GB: u32 = 0x00002000;

    /// Open Flag: Creates a pool memory file of size 1TB
    pub const O_1TB: u32 = 0x00004000;

    /// Open Flag: Creates a pool memory file of size 2TB
    pub const O_2TB: u32 = 0x00008000;

    /// Open Flag: Creates a pool memory file of size 4TB
    pub const O_4TB: u32 = 0x00010000;

    /// Open Flag: Creates a pool memory file of size 8TB
    pub const O_8TB: u32 = 0x00020000;

    /// Open Flag: Creates a pool memory file of size 16TB
    pub const O_16TB: u32 = 0x00040000;

    /// Open Flag: Creates a pool memory file of size 32TB
    pub const O_32TB: u32 = 0x00080000;

    /// Open Flag: Creates a pool memory file of size 64TB
    pub const O_64TB: u32 = 0x00100000;
}

pub use open_flags::*;

/// Shows that the pool has a root object
pub const FLAG_HAS_ROOT: u64 = 0x0000_0001;

/// This macro can be used to declare a static struct for the inner data of an
/// arbitrary allocator.
#[macro_export]
macro_rules! static_inner_object {
    ($id:ident, $ty:ty) => {
        static mut $id: Option<&'static mut $ty> = None;
    };
}

/// This macro can be used to access static data of an arbitrary allocator
#[macro_export]
#[track_caller]
macro_rules! static_inner {
    ($id:ident, $inner:ident, $body:block) => {
        unsafe {
            if let Some($inner) = &mut $id {
                $body
            } else {
                panic!("No memory pool is open");
            }
        }
    };
}

/// Persistent Memory Pool
///
/// This trait can be used to define a persistent memory pool type. The
/// methods of `MemPool` trait do not have a reference to self in order to make
/// sure that all information that it works with, including the virtual address
/// boundaries, are static. Therefore, all objects with the same memory
/// allocator will share a unique memory pool type. Having a strong set of type
/// checking rules, Rust prevents referencing from one memory pool to another.
///
/// To implement a new memory pool, you should define a new type with static
/// values, that implements `MemPool`. You may use [`static_inner_object!()`]
/// to statically define allocator's inner data, and [`static_inner!()`] to
/// access it. You may also use the default allocator using [`pool!()`] which
/// creates a pool module with a default allocator of type [`BuddyAlloc`].
///
/// # Examples
/// The following example shows how to use `MemPool` to track allocations of a
/// single numerical object of type `i32`.
///
/// ```
/// # use corundum::alloc::MemPool;
/// # use corundum::stm::Journal;
/// # use corundum::result::Result;
/// # use std::ops::Range;
/// use std::alloc::{alloc,dealloc,realloc,Layout};
///
/// struct TrackAlloc {}
///
/// unsafe impl MemPool for TrackAlloc {
///     fn rng() -> Range<u64> { 0..u64::MAX }
///     unsafe fn pre_alloc(size: usize) -> (*mut u8, u64, usize, usize) {
///         let p = alloc(Layout::from_size_align_unchecked(size, 4));
///         println!("A block of {} bytes is allocated at {}", size, p as u64);
///         (p, p as u64, size, 0)
///     }
///     unsafe fn pre_dealloc(p: *mut u8, size: usize) -> usize {
///         println!("A block of {} bytes at {} is deallocated", size, p as u64);
///         dealloc(p, Layout::from_size_align_unchecked(size, 1));
///         0
///     }
/// }
///
/// unsafe {
///     let (p, _, _) = TrackAlloc::alloc(1);
///     *p = 10;
///     println!("loc {} contains {}", p as u64, *p);
///     TrackAlloc::dealloc(p, 1);
/// }
/// ```
///
/// # Safety
///
/// This is the developer's responsibility to manually drop allocated objects.
/// One way for memory management is to use pointer wrappers that implement
/// [`Drop`] trait and deallocate the object on drop. Unsafe
/// methods does not guarantee persistent memory safety.
///
/// `pmem` crate provides `Pbox`, `Prc`, and `Parc` for memory management using
/// RAII. They internally use the unsafe methods.
/// 
/// [`pool!()`]: ./default/macro.pool.html
/// [`static_inner_object!()`]: ../macro.static_inner_object.html
/// [`static_inner!()`]: ../macro.static_inner.html
/// [`BuddyAlloc`]: ./default/struct.BuddyAlloc.html
pub unsafe trait MemPool
where
    Self: 'static + Sized,
{
    /// Opens a new pool without any root object. This function is for testing 
    /// and is not useful in real applications as none of the allocated
    /// objects in persistent region is durable. The reason is that they are not
    /// reachable from a root object as it doesn't exists. All objects can live
    /// only in the scope of a transaction.
    /// 
    /// # Flags
    ///   * O_C:    create a memory pool file if not exists
    ///   * O_F:    format the memory pool file
    ///   * O_CNE:  create a memory pool file if not exists
    ///   * O_CF:   create and format a new memory pool file
    ///   * O_CFNE: create and format a memory pool file only if not exists
    /// 
    /// See [`open_flags`](./open_flags/index.html) for more options.
    fn open_no_root(_path: &str, _flags: u32) -> Result<Self> {
        unimplemented!()
    }

    /// Commits all changes and clears the logs for all threads
    ///
    /// This method should be called while dropping the `MemPool` object to
    /// make sure that all uncommitted changes outside transactions, such as
    /// reference counters, are persistent.
    unsafe fn close() -> Result<()> {
        unimplemented!()
    }

    /// Returns the zone index corresponding to a given address
    #[inline]
    fn zone(_off: u64) -> usize {
        0
    }

    /// Opens a pool and retrieves the root object
    ///
    /// The root type should implement [`RootObj`] trait in order to create a
    /// root object on its absence. This function [creates and] returns an
    /// immutable reference to the root object. The pool remains open as long as
    /// the root object is in the scope. Like other persistent objects, the root
    /// object is immutable and it is modifiable via interior mutability.
    /// 
    /// # Flags
    ///   * O_C:    create a memory pool file if not exists
    ///   * O_F:    format the memory pool file
    ///   * O_CNE:  create a memory pool file if not exists
    ///   * O_CF:   create and format a new memory pool file
    ///   * O_CFNE: create and format a memory pool file only if not exists
    /// 
    /// See [`open_flags`](./open_flags/index.html) for more options.
    ///
    /// # Examples
    ///
    /// ```
    /// use corundum::default::*;
    ///
    /// let root = BuddyAlloc::open::<i32>("foo.pool", O_CF).unwrap();
    ///
    /// assert_eq!(*root, i32::default());
    /// ```
    ///
    /// ## Single-thread Shared Root Object
    ///
    /// [`Prc`]`<`[`PCell`]`<T>>` can be used in order to have a mutable shared
    /// root object, as follows.
    ///
    /// ```
    /// use corundum::default::*;
    ///
    /// type Root = Prc<PCell<i32>>;
    ///
    /// let root = BuddyAlloc::open::<Root>("foo.pool", O_CF).unwrap();
    ///
    /// let data = root.get();
    ///
    /// if data == i32::default() {
    ///     println!("Initializing data");
    ///     // This block runs only once to initialize the root object
    ///     transaction(|j| {
    ///         root.set(10, j);
    ///     }).unwrap();
    /// }
    ///
    /// assert_eq!(root.get(), 10);
    /// ```
    ///
    /// ## Thread-safe Root Object
    ///
    /// If you need a thread-safe root object, you may want to wrap the root object
    /// in [`Parc`]`<`[`PMutex`]`<T>>`, as shown in the example below:
    ///
    /// ```
    /// use corundum::default::*;
    /// use std::thread;
    ///
    /// type Root = Parc<PMutex<i32>>;
    ///
    /// let root = BuddyAlloc::open::<Root>("foo.pool", O_CF).unwrap();
    ///
    /// let mut threads = vec!();
    ///
    /// for _ in 0..10 {
    ///     let root = Parc::volatile(&root);
    ///     threads.push(thread::spawn(move || {
    ///         transaction(|j| {
    ///             if let Some(root) = root.upgrade(j) {
    ///                 let mut root = root.lock(j);
    ///                 *root += 10;
    ///             }
    ///         }).unwrap();
    ///     }));
    /// }
    ///
    /// for thread in threads {
    ///     thread.join().unwrap();
    /// }
    ///
    /// transaction(|j| {
    ///     let data = root.lock(j);
    ///     assert_eq!(*data % 100, 0);
    /// }).unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// * A volatile memory pool (e.g. `Heap`) doesn't have a root object.
    /// * The pool should be open before accessing the root object.
    ///
    /// [`RootObj`]: ../stm/trait.RootObj.html
    /// [`Prc`]: ../prc/struct.Prc.html
    /// [`Parc`]: ../sync/parc/struct.Parc.html
    /// [`PCell`]: ./default/type.PCell.html
    /// [`PRefCell`]: ./default/type.PRefCell.html
    /// [`PMutex`]: ./default/type.PMutex.html
    fn open<'a, U: 'a + PSafe + RootObj<Self>>(
        _path: &str,
        _flags: u32,
    ) -> Result<RootCell<'a, U, Self>> {
        unimplemented!()
    }

    /// Formats the memory pool file
    unsafe fn format(_path: &str) -> Result<()> {
        unimplemented!()
    }

    /// Applies open pool flags
    unsafe fn apply_flags(path: &str, flags: u32) -> Result<()> {
        let mut size: u64 = flags as u64 >> 4;
        if size.count_ones() > 1 {
            return Err("Cannot have multiple size flags".to_string());
        } else if size == 0 {
            size = DEFAULT_POOL_SIZE;
        } else {
            if flags & (O_C | O_CNE) == 0 {
                return Err("Cannot use size flag without a create flag".to_string());
            }
            size <<= 30;
        }
        let mut format = !Path::new(path).exists() && ((flags & O_F) != 0);
        if ((flags & O_C) != 0) || ((flags & O_CNE != 0) && !Path::new(path).exists()) {
            let _=std::fs::remove_file(path);
            create_file(path, size)?;
            format = (flags & O_F) != 0;
        }
        if format {
            Self::format(path)?;
        }
        Ok(())
    }

    /// Indicates if the given offset is allocated
    #[inline]
    fn allocated(_off: u64, _len: usize) -> bool {
        true
    }

    /// Translates raw pointers to memory offsets
    ///
    /// # Safety
    ///
    /// The raw pointer should be in the valid range
    #[inline]
    unsafe fn off_unchecked<T: ?Sized>(x: *const T) -> u64 {
        (x as *const u8 as u64) - Self::start()
    }

    /// Acquires a reference pointer to the object
    ///
    /// # Safety
    ///
    /// The offset should be in the valid address range
    #[inline]
    unsafe fn get_unchecked<'a, T: 'a + ?Sized>(off: u64) -> &'a T {
        union U<'b, K: 'b + ?Sized> {
            off: u64,
            raw: &'b K,
        }

        #[cfg(any(feature = "access_violation_check", debug_assertions))]
        assert!( Self::allocated(off, 1), "Bad address (0x{:x})", off );

        U { off: Self::start() + off }.raw
    }

    /// Acquires a mutable reference to the object
    ///
    /// # Safety
    ///
    /// The offset should be in the valid address range
    #[inline]
    #[track_caller]
    unsafe fn get_mut_unchecked<'a, T: 'a + ?Sized>(off: u64) -> &'a mut T {
        union U<'b, K: 'b + ?Sized> {
            off: u64,
            raw: &'b mut K,
        }

        #[cfg(any(feature = "access_violation_check", debug_assertions))]
        assert!( Self::allocated(off, 1), "Bad address (0x{:x})", off );

        U { off: Self::start() + off }.raw
    }

    /// Acquires a reference to the slice
    ///
    /// # Safety
    ///
    /// The offset should be in the valid address range
    #[inline]
    unsafe fn deref_slice_unchecked<'a, T: 'a>(off: u64, len: usize) -> &'a [T] {
        if len == 0 {
            &[]
        } else {
            union U<'b, K: 'b> {
                off: u64,
                raw: &'b K,
            }
            let ptr = U {
                off: Self::start() + off,
            }
            .raw;
            let res = std::slice::from_raw_parts(ptr, len);

            #[cfg(any(feature = "access_violation_check", debug_assertions))]
            assert!(
                Self::allocated(off, mem::size_of::<T>() * len),
                format!(
                    "Bad address (0x{:x}..0x{:x})",
                    off,
                    off + (mem::size_of::<T>() * len) as u64 - 1
                )
            );

            res
        }
    }

    /// Acquires a mutable reference to the slice
    ///
    /// # Safety
    ///
    /// The offset should be in the valid address range
    #[inline]
    unsafe fn deref_slice_unchecked_mut<'a, T: 'a>(off: u64, len: usize) -> &'a mut [T] {
        if len == 0 {
            &mut []
        } else {
            union U<'b, K: 'b> {
                off: u64,
                raw: &'b mut K,
            }
            let ptr = U {
                off: Self::start() + off,
            }
            .raw;
            let res = std::slice::from_raw_parts_mut(ptr, len);

            #[cfg(any(feature = "access_violation_check", debug_assertions))]
            assert!(
                Self::allocated(off, mem::size_of::<T>() * len),
                format!(
                    "Bad address (0x{:x}..0x{:x})",
                    off,
                    off + (mem::size_of::<T>() * len) as u64 - 1
                )
            );

            res
        }
    }

    /// Acquires a reference to the object
    #[inline]
    unsafe fn deref<'a, T: 'a>(off: u64) -> Result<&'a T> {
        if Self::allocated(off, mem::size_of::<T>()) {
            Ok(Self::get_unchecked(off))
        } else {
            Err(format!("Bad address (0x{:x})", off))
        }
    }

    /// Acquires a mutable reference pointer to the object
    #[inline]
    unsafe fn deref_mut<'a, T: 'a>(off: u64) -> Result<&'a mut T> {
        if Self::allocated(off, mem::size_of::<T>()) {
            Ok(Self::get_mut_unchecked(off))
        } else {
            Err(format!("Bad address (0x{:x})", off))
        }
    }

    /// Translates raw pointers to memory offsets
    #[inline]
    fn off<T: ?Sized>(x: *const T) -> Result<u64> {
        if Self::valid(unsafe { &*x }) {
            Ok(x as *const u8 as u64 - Self::start())
        } else {
            Err("out of valid range".to_string())
        }
    }

    /// Valid Virtual Address Range
    fn rng() -> Range<u64> {
        Self::start()..Self::end()
    }

    /// Start of virtual address range
    #[inline]
    fn start() -> u64 {
        Self::rng().start
    }

    /// End of virtual address range
    #[inline]
    fn end() -> u64 {
        Self::rng().end
    }

    /// Total size of the memory pool
    fn size() -> usize {
        unimplemented!()
    }

    /// Available space in the pool
    fn available() -> usize {
        unimplemented!()
    }

    /// Total occupied space
    fn used() -> usize {
        Self::size() - Self::available()
    }

    /// Checks if the reference `p` belongs to this pool
    #[inline]
    fn valid<T: ?Sized>(p: &T) -> bool {
        let rng = Self::rng();
        let start = p as *const T as *const u8 as u64;
        // let end = start + std::mem::size_of_val(p) as u64;
        start >= rng.start && start < rng.end
        // && end >= rng.start && end < rng.end
    }

    /// Checks if `addr` is in the valid address range if this allocator
    ///
    /// `addr` contains the scalar of a virtual address. If you have a raw
    /// fat pointer of type T, you can obtain its virtual address by converting
    /// it into a thin pointer and then `u64`.
    ///
    /// # Examples
    ///
    /// ```
    /// let p = Box::new(1);
    /// println!("Address {:#x} contains value '{}'", p.as_ref() as *const _ as u64, *p);
    /// ```
    #[inline]
    fn contains(addr: u64) -> bool {
        let rng = Self::rng();
        addr >= rng.start && addr < rng.end
    }

    /// Allocate memory as described by the given `layout`.
    ///
    /// Returns a pointer to newly-allocated memory.
    ///
    /// # Safety
    ///
    /// This function is unsafe because undefined behavior can result
    /// if the caller does not ensure that `layout` has non-zero size.
    /// The allocated block of memory may or may not be initialized.
    #[inline]
    #[track_caller]
    unsafe fn alloc(size: usize) -> (*mut u8, u64, usize) {
        let (p, off, len, z) = Self::pre_alloc(size);
        Self::drop_on_failure(off, len, z);
        Self::perform(z);
        (p, off, len)
    }

    /// Deallocate the block of memory at the given `ptr` pointer with the
    /// given `size`.
    ///
    /// # Safety
    ///
    /// This function is unsafe because undefined behavior can result if the
    /// caller does not ensure all of the following:
    ///
    /// * `ptr` must denote a block of memory currently allocated via this
    ///   allocator,
    ///
    /// * `size` must be the same size that was used to allocate that block
    ///   of memory.
    #[inline]
    #[track_caller]
    unsafe fn dealloc(ptr: *mut u8, size: usize) {
        Self::perform(Self::pre_dealloc(ptr, size));
    }

    /// Prepares allocation without performing it
    /// 
    /// This function is used internally for low-level atomicity in memory
    /// allocation. See [`Log::set()`] for more details.
    /// 
    /// It returns a 4-tuple:
    ///     1. Raw pointer
    ///     2. Offset
    ///     3. Size
    ///     4. Zone index
    /// 
    /// # Examples
    /// 
    /// ```
    /// # use corundum::default::*;
    /// # type P = BuddyAlloc;
    /// # let _=P::open_no_root("foo.pool", O_CF).unwrap();
    /// unsafe {
    ///     let (ptr, _, _, z) = P::pre_alloc(8);
    ///     *ptr = 10;
    ///     P::perform(z);
    /// }
    /// ```
    /// 
    /// [`Log::set()`]: ../stm/struct.Log.html#method.set
    /// 
    unsafe fn pre_alloc(size: usize) -> (*mut u8, u64, usize, usize);

    /// Prepares deallocation without performing it
    /// 
    /// This function is used internally for low-level atomicity in memory
    /// allocation. See [`Log::set()`] for more details.
    /// 
    /// It returns the zone in which the deallocation happens.
    /// 
    /// # Examples
    /// 
    /// ```
    /// # use corundum::default::*;
    /// # type P = BuddyAlloc;
    /// # let _=P::open_no_root("foo.pool", O_CF).unwrap();
    /// unsafe {
    ///     let (ptr, _, _) = P::alloc(8);
    ///     *ptr = 10;
    ///     let zone = P::pre_dealloc(ptr, 8);
    ///     assert_eq!(*ptr, 10);
    ///     P::perform(zone);
    ///     assert_ne!(*ptr, 10);
    /// }
    /// ```
    /// 
    /// [`Log::set()`]: ../stm/struct.Log.html#method.set
    /// 
    unsafe fn pre_dealloc(ptr: *mut u8, size: usize) -> usize;

    /// Adds a low-level log to update as 64-bit `obj` to `val` when 
    /// [`perform()`] is called. See [`Log::set()`] for more details.
    /// 
    /// [`perform()`]: #method.perform
    /// [`Log::set()`]: ../stm/struct.Log.html#method.set
    /// 
    unsafe fn log64(_off: u64, _val: u64, _zone: usize) {
        unimplemented!()
    }

    /// Adds a low-level `DropOnFailure` log to perform inside the allocator. 
    /// This is internally used to atomically allocate a new objects. Calling
    /// [`perform()`] drops these logs.
    /// 
    /// # Examples
    /// 
    /// ```
    /// # use corundum::default::*;
    /// # type P = BuddyAlloc;
    /// # let _ = P::open_no_root("foo.pool", O_CF).unwrap();
    /// unsafe {
    ///     // Prepare an allocation. The allocation is not durable yet. In case
    ///     // of a crash, the prepared allocated space is gone. It is fine
    ///     // because it has not been used. The `pre_` and `perform` functions
    ///     // form a low-level atomic section.
    ///     let (obj, off, len, zone) = P::pre_alloc(1);
    /// 
    ///     // Create a low-level DropOnFailure log. This log is going to be used
    ///     // when a crash happens while performing the changes made by the
    ///     // preparation functions. If a crash happens before that, these logs
    ///     // will be discarded.
    ///     P::drop_on_failure(off, len, zone);
    ///     
    ///     // It is fine to work with the prepared raw pointer. All changes in
    ///     // the low-level atomic section are considered as part of the
    ///     // allocation and will be gone in case of a crash, as the allocation
    ///     // will be dropped.
    ///     *obj = 20;
    /// 
    ///     // Transaction ends here. The perform function sets the `operating`
    ///     // flag to show that the prepared changes are being materialized.
    ///     // This flag remains set until the end of materialization. In case
    ///     // of a crash while operating, the recovery procedure first continues
    ///     // the materialization, and then uses the `DropOnFailure` logs to
    ///     // reclaim the allocation. `perform` function realizes the changes
    ///     // made by the `pre_` function on the given memory zone.
    ///     P::perform(zone);
    /// }
    /// ```
    /// 
    /// [`perform()`]: #method.perform
    /// [`Journal`]: ../stm/journal/struct.Journal.html
    /// 
    unsafe fn drop_on_failure(_off: u64, _len: usize, _zone: usize) {}

    /// Performs the prepared operations
    /// 
    /// It materializes the changes made by [`pre_alloc`](#method.pre_alloc),
    /// [`pre_dealloc`](#method.pre_dealloc), and
    /// [`pre_realloc`](#method.pre_realloc). See [`Log::set()`] for more
    /// details.
    /// 
    /// [`Log::set()`]: ../stm/struct.Log.html#method.set
    /// 
    unsafe fn perform(_zone: usize) { }

    /// Discards the prepared operations
    /// 
    /// Discards the changes made by [`pre_alloc`](#method.pre_alloc),
    /// [`pre_dealloc`](#method.pre_dealloc), and
    /// [`pre_realloc`](#method.pre_realloc).  See [`Log::set()`] for more
    /// details.
    /// 
    /// [`Log::set()`]: ../stm/struct.Log.html#method.set
    /// 
    unsafe fn discard(_zone: usize) { }

    /// Behaves like `alloc`, but also ensures that the contents
    /// are set to zero before being returned.
    ///
    /// # Safety
    ///
    /// This function is unsafe for the same reasons that `alloc` is.
    /// However the allocated block of memory is guaranteed to be initialized.
    ///
    /// # Errors
    ///
    /// Returning a null pointer indicates that either memory is exhausted
    /// or `layout` does not meet allocator's size or alignment constraints,
    /// just as in `alloc`.
    ///
    /// Clients wishing to abort computation in response to an
    /// allocation error are encouraged to call the [`handle_alloc_error`] function,
    /// rather than directly invoking `panic!` or similar.
    ///
    /// [`handle_alloc_error`]: ../../alloc/alloc/fn.handle_alloc_error.html
    unsafe fn alloc_zeroed(size: usize) -> *mut u8 {
        let (ptr, _, _) = Self::alloc(size);
        if !ptr.is_null() {
            std::ptr::write_bytes(ptr, 0, size);
        }
        ptr
    }

    /// Allocates new memory and then places `x` into it with `DropOnFailure` log
    unsafe fn new<'a, T: PSafe + 'a>(x: T, j: &Journal<Self>) -> &'a mut T {
        debug_assert!(mem::size_of::<T>() != 0, "Cannot allocated ZST");

        let mut log = Log::drop_on_failure(u64::MAX, 1, j);
        let (p, off, len, z) = Self::atomic_new(x);
        log.set(off, len, z);
        Self::perform(z);
        p
    }

    /// Allocates a new slice and then places `x` into it with `DropOnAbort` log
    unsafe fn new_slice<'a, T: PSafe + 'a>(x: &'a [T], _journal: &Journal<Self>) -> &'a mut [T] {
        debug_assert!(mem::size_of::<T>() != 0, "Cannot allocate ZST");
        debug_assert!(!x.is_empty(), "Cannot allocate empty slice");

        let mut log = Log::drop_on_abort(u64::MAX, 1, _journal);
        let (p, off, size, z) = Self::atomic_new_slice(x);
        log.set(off, size, z);
        Self::perform(z);
        p
    }

    /// Allocates new memory and then places `x` into it without realizing the allocation
    unsafe fn atomic_new<'a, T: 'a>(x: T) -> (&'a mut T, u64, usize, usize) {
        union U<'b, K: 'b + ?Sized> {
            raw: *mut u8,
            rf: &'b mut K,
        }

        #[cfg(feature = "verbose")]
        println!("          ALLOC      TYPE: {}", std::any::type_name::<T>());

        let size = mem::size_of::<T>();
        let (raw, off, len, z) = Self::pre_alloc(size);
        if raw.is_null() {
            panic!("Memory exhausted");
        }
        Self::drop_on_failure(off, len, z);
        let p = U { raw }.rf;
        mem::forget(ptr::replace(p, x));
        (p, off, size, z)
    }

    /// Allocates new memory and then places `x` into it without realizing the allocation
    unsafe fn atomic_new_slice<'a, T: 'a + PSafe>(x: &'a [T]) -> (&'a mut [T], u64, usize, usize) {
        #[cfg(feature = "verbose")]
        println!(
            "          ALLOC      TYPE: [{}; {}]",
            std::any::type_name::<T>(),
            x.len()
        );

        let (ptr, off, size, z) = Self::pre_alloc(Layout::for_value(x).size());
        if ptr.is_null() {
            panic!("Memory exhausted");
        }
        Self::drop_on_failure(off, size, z);
        ptr::copy_nonoverlapping(
            x as *const _ as *const u8,
            ptr,
            x.len() * mem::size_of::<T>(),
        );
        (
            std::slice::from_raw_parts_mut(ptr.cast(), x.len()),
            off,
            size,
            z
        )
    }

    /// Allocates new memory without copying data
    unsafe fn new_uninit<'a, T: PSafe + 'a>(j: &Journal<Self>) -> &'a mut T {
        let mut log = Log::drop_on_failure(u64::MAX, 1, j);
        let (p, off, size, z) = Self::atomic_new_uninit();
        Self::drop_on_failure(off, size, z);
        log.set(off, size, z);
        Self::perform(z);
        p
    }

    /// Allocates new memory without copying data
    unsafe fn new_uninit_for_layout(size: usize, journal: &Journal<Self>) -> *mut u8 {
        #[cfg(feature = "verbose")]
        println!("          ALLOC      {:?}", size);

        let mut log = Log::drop_on_abort(u64::MAX, 1, journal);
        let (p, off, len, z) = Self::pre_alloc(size);
        if p.is_null() {
            panic!("Memory exhausted");
        }
        Self::drop_on_failure(off, len, z);
        log.set(off, len, z);
        Self::perform(z);
        p
    }

    /// Allocates new memory without copying data and realizing the allocation
    unsafe fn atomic_new_uninit<'a, T: 'a>() -> (&'a mut T, u64, usize, usize) {
        union U<'b, K: 'b + ?Sized> {
            ptr: *mut u8,
            rf: &'b mut K,
        }

        let (ptr, off, len, z) = Self::pre_alloc(mem::size_of::<T>());
        if ptr.is_null() {
            panic!("Memory exhausted");
        }
        Self::drop_on_failure(off, len, z);
        (U { ptr }.rf, off, len, z)
    }

    /// Allocates new memory for value `x`
    unsafe fn alloc_for_value<'a, T: ?Sized>(x: &T) -> &'a mut T {
        union U<'b, K: 'b + ?Sized> {
            raw: *mut u8,
            rf: &'b mut K,
        }
        let raw = Self::alloc(mem::size_of_val(x));
        if raw.0.is_null() {
            panic!("Memory exhausted");
        }
        U { raw: raw.0 }.rf
    }

    /// Creates a `DropOnCommit` log for the value `x`
    unsafe fn free<'a, T: PSafe + ?Sized>(x: &mut T) {
        // std::ptr::drop_in_place(x);
        let off = Self::off_unchecked(x);
        let len = mem::size_of_val(x);
        if std::thread::panicking() {
            Log::drop_on_abort(off, len, &mut Journal::<Self>::current(true).unwrap().0);
        } else {
            Log::drop_on_commit(off, len, &mut Journal::<Self>::current(true).unwrap().0);
        }
    }

    /// Creates a `DropOnCommit` log for the value `x`
    unsafe fn free_slice<'a, T: PSafe>(x: &mut [T]) {
        // eprintln!("FREEING {} of size {}", x as *mut u8 as u64, len);
        if x.len() > 0 {
            let off = Self::off_unchecked(x);
            Log::drop_on_commit(
                off,
                x.len() * mem::size_of::<T>(),
                &mut Journal::<Self>::current(true).unwrap().0,
            );
        }
    }

    /// Frees the allocation for value `x` immediately
    unsafe fn free_nolog<'a, T: ?Sized>(x: &T) {
        Self::perform(
            Self::pre_dealloc(x as *const _ as *mut u8, mem::size_of_val(x))
        );
    }

    /// Executes a closure guarded by a global mutex
    unsafe fn guarded<T, F: FnOnce() -> T>(f: F) -> T {
        f()
    }

    /// Creates a new `Journal` object for the current thread
    unsafe fn new_journal(_tid: ThreadId) { }

    /// Drops a `journal` from memory
    unsafe fn drop_journal(_journal: &mut Journal<Self>) { }

    /// Returns the list of all journals
    unsafe fn journals() -> &'static mut HashMap<ThreadId, (&'static Journal<Self>, i32)> {
        unimplemented!()
    }

    /// Recovers from a crash
    unsafe fn recover() {
        unimplemented!()
    }

    /// Commits all changes and clears the logs for one thread
    ///
    /// If the transaction is nested, it postpones the commit to the top most
    /// transaction.
    ///
    /// # Safety
    ///
    /// This function is for internal use and should not be called elsewhere.
    ///
    #[inline]
    unsafe fn commit() {
        // Self::discard(crate::ll::cpu());
        if let Some(journal) = Journal::<Self>::current(false) {
            journal.1 -= 1;

            if journal.1 == 0 {
                #[cfg(feature = "verbose")]
                println!("{:?}", journal.0);

                let journal = as_mut(journal.0);
                journal.commit();
                journal.clear();
            }
        }
    }

    #[inline]
    /// Commits all changes without clearing the logs
    ///
    /// If the transaction is nested, it postpones the commit to the top most
    /// transaction.
    ///
    /// # Safety
    ///
    /// This function is for internal use and should not be called elsewhere.
    ///
    unsafe fn commit_no_clear() {
        // Self::discard(crate::ll::cpu());
        if let Some(journal) = Journal::<Self>::current(false) {
            if journal.1 == 1 {
                #[cfg(feature = "verbose")]
                println!("{:?}", journal.0);

                as_mut(journal.0).commit();
            }
        }
    }

    #[inline]
    /// Clears the logs
    ///
    /// If the transaction is nested, it postpones the clear to the top most
    /// transaction.
    ///
    /// # Safety
    ///
    /// This function is for internal use and should not be called elsewhere.
    ///
    unsafe fn clear() {
        if let Some(journal) = Journal::<Self>::current(false) {
            journal.1 -= 1;

            if journal.1 == 0 {
                #[cfg(feature = "verbose")]
                println!("{:?}", journal.0);

                as_mut(journal.0).clear();
            }
        }
    }

    #[inline]
    /// Discards all changes and clears the logs
    ///
    /// If the transaction is nested, it propagates the panic upto the top most
    /// transaction to make all of them tainted.
    ///
    /// # Safety
    ///
    /// This function is for internal use and should not be called elsewhere.
    ///
    unsafe fn rollback() {
        // Self::discard(crate::ll::cpu());
        if let Some(journal) = Journal::<Self>::current(false) {
            journal.1 -= 1;

            if journal.1 == 0 {
                #[cfg(feature = "verbose")]
                println!("{:?}", journal.0);

                let journal = as_mut(journal.0);
                journal.rollback();
                journal.clear();
            } else {
                // Propagate the panic to the upper transactions
                panic!("Unsuccessful nested transaction");
            }
        }
    }

    #[inline]
    /// Discards all changes without clearing the logs
    ///
    /// If the transaction is nested, it propagates the panic upto the top most
    /// transaction to make all of them tainted.
    ///
    /// # Safety
    ///
    /// This function is for internal use and should not be called elsewhere.
    ///
    unsafe fn rollback_no_clear() {
        if let Some(journal) = Journal::<Self>::current(false) {
            if journal.1 == 1 {
                #[cfg(feature = "verbose")]
                println!("{:?}", journal.0);

                as_mut(journal.0).rollback();
            } else {
                // Propagate the panic to the upper transactions
                panic!("Unsuccessful nested transaction");
            }
        }
    }

    /// Executes commands atomically
    /// 
    /// The `transaction` function takes a closure with one argument of type
    /// `&Journal<Self>`. Before running the closure, it atomically creates a
    /// [`Journal`] object, if required, and prepares an immutable reference to
    /// it. Since there is no other safe way to create a `Journal` object, it
    /// ensures that every function taking an argument of type `&Journal<P>` is
    /// enforced to be invoked from a transaction.
    /// 
    /// The captured types are bounded to be [`TxInSafe`], unless explicitly
    /// asserted otherwise using [`AssertTxInSafe`] type wrapper. This
    /// guarantees the volatile state consistency, as well as the persistent
    /// state.
    /// 
    /// The returned type should be [`TxOutSafe`]. This prevents sending out
    /// unreachable persistent objects. The only way out of a transaction for
    /// a persistent object is to be reachable by the root object.
    ///
    /// # Examples
    /// 
    /// ```
    /// use corundum::default::*;
    /// 
    /// type P = BuddyAlloc;
    /// 
    /// let root = P::open::<PCell<i32>>("foo.pool", O_CF).unwrap();
    /// 
    /// let old = root.get();
    /// let new = BuddyAlloc::transaction(|j| {
    ///     root.set(root.get() + 1, j);
    ///     root.get()
    /// }).unwrap();
    /// 
    /// assert_eq!(new, old + 1);
    /// ```
    /// 
    /// [`Journal`]: ../stm/journal/struct.Journal.html
    /// [`TxInSafe`]: ../trait.TxInSafe.html
    /// [`TxOutSafe`]: ../trait.TxOutSafe.html
    /// [`AssertTxInSafe`]: ../struct.AssertTxInSafe.html
    /// 
    #[inline]
    fn transaction<T, F: FnOnce(&Journal<Self>) -> T>(body: F) -> Result<T>
    where
        F: TxInSafe + UnwindSafe,
        T: TxOutSafe,
    {
        let mut chaperoned = false;
        let cptr = &mut chaperoned as *mut bool;
        let res = std::panic::catch_unwind(move || {
            let chaperon = Chaperon::current();
            if let Some(ptr) = chaperon {
                // FIXME: Chaperone session is corrupted. fix it.
                unsafe {
                    *cptr = true;
                    let mut chaperon = &mut *ptr;
                    chaperon.postpone(
                        &|| Self::commit_no_clear(),
                        &|| Self::rollback_no_clear(),
                        &|| Self::clear(),
                    );
                    body({
                        let j = Journal::<Self>::current(true).unwrap();
                        j.1 += 1;
                        let journal = as_mut(j.0);
                        journal.start_session(&mut chaperon);
                        journal.reset(JOURNAL_COMMITTED);
                        journal
                    })
                }
            } else {
                body({
                    let j = Journal::<Self>::current(true).unwrap();
                    j.1 += 1;
                    as_mut(j.0).reset(JOURNAL_COMMITTED);
                    j.0
                })
            }
        });
        unsafe {
            if let Ok(res) = res {
                if !chaperoned {
                    Self::commit();
                }
                Ok(res)
            } else {
                if !chaperoned {
                    Self::rollback();
                    Err("Unsuccessful transaction".to_string())
                } else {
                    // Propagates the panic to the top level in enforce rollback
                    panic!("Unsuccessful chaperoned transaction");
                }
            }
        }
    }

    fn gen() -> u32 {
        0
    }

    /// Prints memory information
    fn print_info() {}

    #[cfg(feature = "capture_footprint")]
    fn footprint() -> usize {
        0
    }
}

pub(crate) fn create_file(filename: &str, size: u64) -> Result<()> {
    let file = OpenOptions::new().write(true).create(true).open(filename);
    if file.is_err() {
        Err(format!("{}", file.err().unwrap()))
    } else {
        if let Some(e) = file.unwrap().set_len(size).err() {
            Err(format!("{}", e))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod test {
    use crate::default::*;

    #[test]
    #[ignore]
    fn nested_transactions() {
        let _image = BuddyAlloc::open_no_root("nosb.pool", O_CFNE);
        if let Err(e) = BuddyAlloc::transaction(|_| {
            let _ = BuddyAlloc::transaction(|_| {
                let _ = BuddyAlloc::transaction(|_| {
                    let _ = BuddyAlloc::transaction(|_| {
                        println!("should print");
                        panic!("intentional");
                    });
                    println!("should not print");
                });
                println!("should not print");
            });
            println!("should not print");
        }) {
            println!("Error: '{}'", e);
        }
    }
}