
use std::marker::PhantomData;
use std::mem;
use std::ops::Add;
use std::fmt;

/// Represents a pointer to a field of type `U` within the type `T`
#[repr(transparent)]
pub struct FieldOffset<T, U>(
    /// Offset in bytes of the field within the struct
    usize,
    /// A pointer-to-member can be thought of as a function from
    /// `&T` to `&U` with matching lifetimes
    PhantomData<dyn for<'a> Fn(&'a T) -> &'a U>
);

impl<T, U> FieldOffset<T, U> {
    // Use MaybeUninit to get a fake T
    #[cfg(fieldoffset_maybe_uninit)]
    #[inline]
    fn with_uninit_ptr<R, F: FnOnce(*const T) -> R>(f: F) -> R {
        let uninit = mem::MaybeUninit::<T>::uninit();
        f(uninit.as_ptr())
    }

    // Use a dangling pointer to get a fake T
    #[cfg(not(fieldoffset_maybe_uninit))]
    #[inline]
    fn with_uninit_ptr<R, F: FnOnce(*const T) -> R>(f: F) -> R {
        f(mem::align_of::<T>() as *const T)
    }

    /// Construct a field offset via a lambda which returns a reference
    /// to the field in question.
    ///
    /// The lambda *must not* access the value passed in.
    pub unsafe fn new<F: for<'a> FnOnce(*const T) -> *const U>(f: F) -> Self {
        let offset = Self::with_uninit_ptr(|base_ptr| {
            let field_ptr = f(base_ptr);
            (field_ptr as usize).wrapping_sub(base_ptr as usize)
        });

        // Construct an instance using the offset
        Self::new_from_offset(offset)
    }
    /// Construct a field offset directly from a byte offset.
    #[inline]
    pub unsafe fn new_from_offset(offset: usize) -> Self {
        // Sanity check: ensure that the field offset plus the field size
        // is no greater than the size of the containing struct. This is
        // not sufficient to make the function *safe*, but it does catch
        // obvious errors like returning a reference to a boxed value,
        // which is owned by `T` and so has the correct lifetime, but is not
        // actually a field.
        assert!(offset + mem::size_of::<U>() <= mem::size_of::<T>());

        FieldOffset(offset, PhantomData)
    }
    // Methods for applying the pointer to member
    /// Apply the field offset to a native pointer.
    #[inline]
    pub fn apply_ptr<'a>(&self, x: *const T) -> *const U {
        ((x as usize) + self.0) as *const U
    }
    /// Apply the field offset to a native mutable pointer.
    #[inline]
    pub fn apply_ptr_mut<'a>(&self, x: *mut T) -> *mut U {
        ((x as usize) + self.0) as *mut U
    }
    /// Apply the field offset to a reference.
    #[inline]
    pub fn apply<'a>(&self, x: &'a T) -> &'a U {
        unsafe { &*self.apply_ptr(x) }
    }
    /// Apply the field offset to a mutable reference.
    #[inline]
    pub fn apply_mut<'a>(&self, x: &'a mut T) -> &'a mut U {
        unsafe { &mut *self.apply_ptr_mut(x) }
    }
    /// Get the raw byte offset for this field offset.
    #[inline]
    pub fn get_byte_offset(&self) -> usize {
        self.0
    }
    // Methods for unapplying the pointer to member
    /// Unapply the field offset to a native pointer.
    ///
    /// *Warning: very unsafe!*
    #[inline]
    pub unsafe fn unapply_ptr<'a>(&self, x: *const U) -> *const T {
        ((x as usize) - self.0) as *const T
    }
    /// Unapply the field offset to a native mutable pointer.
    ///
    /// *Warning: very unsafe!*
    #[inline]
    pub unsafe fn unapply_ptr_mut<'a>(&self, x: *mut U) -> *mut T {
        ((x as usize) - self.0) as *mut T
    }
    /// Unapply the field offset to a reference.
    ///
    /// *Warning: very unsafe!*
    #[inline]
    pub unsafe fn unapply<'a>(&self, x: &'a U) -> &'a T {
        &*self.unapply_ptr(x)
    }
    /// Unapply the field offset to a mutable reference.
    ///
    /// *Warning: very unsafe!*
    #[inline]
    pub unsafe fn unapply_mut<'a>(&self, x: &'a mut U) -> &'a mut T {
        &mut *self.unapply_ptr_mut(x)
    }
}

/// Allow chaining pointer-to-members.
///
/// Applying the resulting field offset is equivalent to applying the first
/// field offset, then applying the second field offset.
///
/// The requirements on the generic type parameters ensure this is a safe operation.
impl<T, U, V> Add<FieldOffset<U, V>> for FieldOffset<T, U> {
    type Output = FieldOffset<T, V>;

    #[inline]
    fn add(self, other: FieldOffset<U, V>) -> FieldOffset<T, V> {
        FieldOffset(self.0 + other.0, PhantomData)
    }
}

/// The debug implementation prints the byte offset of the field in hexadecimal.
impl<T, U> fmt::Debug for FieldOffset<T, U> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "FieldOffset({:#x})", self.0)
    }
}

impl<T, U> Copy for FieldOffset<T, U> { }
impl<T, U> Clone for FieldOffset<T, U> {
    fn clone(&self) -> Self { *self }
}

/// This macro allows safe construction of a FieldOffset,
/// by generating a known to be valid lambda to pass to the
/// constructor. It takes a type and the identifier of a field
/// within that type as input.
///
/// Examples:
///
/// Offset of field `Foo().bar`
///
/// `offset_of!(Foo => bar)`
///
/// Offset of nested field `Foo().bar.x`
///
/// `offset_of!(Foo => bar: Bar => x)`
#[macro_export]
macro_rules! offset_of {
    ($t: tt => $f: tt) => {{
        // Make sure the field exists, and is not being accessed via Deref.
        let $t { $f: _, .. };

        // Construct the offset
        #[allow(unused_unsafe)]
        unsafe {
            $crate::FieldOffset::<$t, _>::new(|x| {
                // This is UB unless/until the compiler special-cases it to
                // not enforce the validity constraint on `x`.
                &(*x).$f as *const _
            })
        }
    }};
    ($t: path => $f: ident: $($rest: tt)*) => {
        offset_of!($t => $f) + offset_of!($($rest)*)
    };
}

#[cfg(test)]
mod tests {
    // Example structs
    #[derive(Debug)]
    struct Foo {
        a: u32,
        b: f64,
        c: bool
    }

    #[derive(Debug)]
    struct Bar {
        x: u32,
        y: Foo,
    }

    #[derive(Debug)]
    struct Tuple(i32, f64);

    #[test]
    fn test_simple() {
        // Get a pointer to `b` within `Foo`
        let foo_b = offset_of!(Foo => b);

        // Construct an example `Foo`
        let mut x = Foo {
            a: 1,
            b: 2.0,
            c: false
        };

        // Apply the pointer to get at `b` and read it
        {
            let y = foo_b.apply(&x);
            assert!(*y == 2.0);
        }

        // Apply the pointer to get at `b` and mutate it
        {
            let y = foo_b.apply_mut(&mut x);
            *y = 42.0;
        }
        assert!(x.b == 42.0);
    }

    #[test]
    fn test_tuple() {
        // Get a pointer to `b` within `Foo`
        let tuple_1 = offset_of!(Tuple => 1);

        // Construct an example `Foo`
        let mut x = Tuple(1, 42.0);

        // Apply the pointer to get at `b` and read it
        {
            let y = tuple_1.apply(&x);
            assert!(*y == 42.0);
        }

        // Apply the pointer to get at `b` and mutate it
        {
            let y = tuple_1.apply_mut(&mut x);
            *y = 5.0;
        }
        assert!(x.1 == 5.0);
    }

    #[test]
    fn test_nested() {
        // Construct an example `Foo`
        let mut x = Bar {
            x: 0,
            y: Foo {
                a: 1,
                b: 2.0,
                c: false
            }
        };

        // Combine the pointer-to-members
        let bar_y_b = offset_of!(Bar => y: Foo => b);

        // Apply the pointer to get at `b` and mutate it
        {
            let y = bar_y_b.apply_mut(&mut x);
            *y = 42.0;
        }
        assert!(x.y.b == 42.0);
    }
}
