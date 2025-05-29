#[macro_export]
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

#[macro_export]
macro_rules! mk_buf {
    [ $ty:ty , $filler:expr  ;$size:expr ] => {
        $crate::mk_static!([$ty; $size], [$filler; $size])
    };
}

#[macro_export]
macro_rules! mk_ch {
    ($val:literal) => {{
        let ch = &*$crate::mk_static! {
            embassy_sync::channel::Channel::<NoopRawMutex, BytesMut, $val>,
            embassy_sync::channel::Channel::new()
        };
        let sender = ch.sender();
        let receiver = ch.receiver();
        (sender, receiver)
    }};

    ($val:literal; $ty: ty) => {{
        let ch = &*$crate::mk_static! {
            embassy_sync::channel::Channel::<NoopRawMutex, $ty, $val>,
            embassy_sync::channel::Channel::new()
        };
        let sender = ch.sender();
        let receiver = ch.receiver();
        (sender, receiver)
    }};
}
