use core::slice;

use bytes::BytesMut;

pub trait BytesMutExtend {
    fn transmute<T>(&self) -> &[T];
    fn transmute_cap<T>(&mut self) -> &mut [T];
}

impl BytesMutExtend for BytesMut {
    fn transmute<T>(&self) -> &[T] {
        debug_assert!(self.len() % core::mem::size_of::<T>() == 0);
        unsafe {
            slice::from_raw_parts(
                self.as_ptr() as *const T,
                self.len() / core::mem::size_of::<T>(),
            )
        }
    }

    fn transmute_cap<T>(&mut self) -> &mut [T] {
        let capacity = self.capacity();
        debug_assert!(capacity % core::mem::size_of::<T>() == 0);
        unsafe {
            slice::from_raw_parts_mut(
                self.as_mut_ptr() as *mut T,
                capacity / core::mem::size_of::<T>(),
            )
        }
    }
}
