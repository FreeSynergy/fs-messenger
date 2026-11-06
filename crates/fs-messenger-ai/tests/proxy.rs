//! A proxy in the environment gets nothing: text meant for a loopback address must not leave by one, and
//! the key must not reach one. A file of its own, with one test, because the environment is the process's.

mod common;

use common::World;
use common::wire::{llama, serve, stream};

#[tokio::test]
async fn the_proxy_settings_of_the_environment_are_ignored() {
    let proxy = serve(vec![stream(llama::answer(&["through the proxy"]))]).await;
    // SAFETY: the only test in this binary, before anything reads the environment on another thread.
    unsafe {
        for name in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            std::env::set_var(name, &proxy.origin);
        }
        std::env::remove_var("NO_PROXY");
        std::env::remove_var("no_proxy");
    }
    let world = World::invited().await;
    let server = serve(vec![stream(llama::answer(&["direct"]))]).await;
    let provider = world.llama(world.config(&server.origin)).await;
    let done = world.turn(&provider, "hello").await;
    assert!(done.sent(), "{:?}", done.live.failure);
    assert_eq!(done.answer().body_text, "direct");
    assert_eq!(proxy.requests().len(), 0);
    assert_eq!(server.requests().len(), 1);
}
