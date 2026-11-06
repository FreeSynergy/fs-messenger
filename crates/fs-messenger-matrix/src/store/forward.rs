//! The part of a store trait that only passes on to the SDK's memory store.
//!
//! Pattern: Decorator, the plain half: these two traits have nearly a hundred methods, most of them
//! reads. A macro keeps them readable as a list. It wraps the whole `impl` so that `async_trait` sees
//! the expanded methods.

/// `impl $store for $ty`, with `explicit` methods written out and the `forward` ones passed on.
macro_rules! decorate_memory {
    (
        impl $store:path, for $ty:ty, error $err:ty;
        explicit { $($explicit:tt)* }
        forward { $( fn $name:ident($($arg:ident : $aty:ty),*) -> $ret:ty; )* }
    ) => {
        #[async_trait::async_trait]
        impl $store for $ty {
            type Error = $err;

            $($explicit)*

            $(
                async fn $name(&self, $($arg: $aty),*) -> Result<$ret, $err> {
                    self.memory.$name($($arg),*).await.map_err(Into::into)
                }
            )*
        }
    };
}

pub(crate) use decorate_memory;
