//! Server-sent events: a response body that keeps arriving while the handler keeps producing it.
//!
//! The shape under test is the one deno's documentation shows and every SSE handler writes: a
//! ReadableStream whose interval enqueues chunks, a request.signal listener that stops the interval
//! when the client goes away, and Response headers of text/event-stream.

mod common;

use std::time::Duration;

use common::{count, start, Client};

const SSE: &str = r#"
let ticks = 0;
let aborts = 0;

Xmas.serve(function (request) {
  const url = new URL(request.url);

  if (url.pathname === '/stats') {
    return new Response(JSON.stringify({ ticks: ticks, aborts: aborts }), {
      headers: { 'content-type': 'application/json' },
    });
  }

  if (url.pathname === '/once') {
    const stream = new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode('data: one\n\n'));
        controller.enqueue(new TextEncoder().encode('data: two\n\n'));
        controller.close();
      },
    });
    return new Response(stream, { headers: { 'content-type': 'text/event-stream' } });
  }

  const stream = new ReadableStream({
    start(controller) {
      const encoder = new TextEncoder();
      const timer = setInterval(() => {
        ticks = ticks + 1;
        controller.enqueue(encoder.encode('data: ' + ticks + '\n\n'));
      }, 10);
      request.signal.addEventListener('abort', () => {
        aborts = aborts + 1;
        clearInterval(timer);
        controller.close();
      });
    },
  });

  return new Response(stream, {
    headers: { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' },
  });
});
"#;

#[test]
fn chunks_arrive_while_the_handler_is_still_producing_them() {
    let server = start(SSE);
    let mut client = Client::to(&server);
    client.send("GET /events HTTP/1.1\r\nHost: t\r\n\r\n");

    // Wait for the second chunk. A buffered answer would only be written once the handler was
    // finished, and this handler is not finished, so the second chunk arriving at all is the proof
    // that the body is going out as it is produced.
    let text = client.read_until("data: 2", Duration::from_secs(5));
    let lower = text.to_ascii_lowercase();
    assert!(lower.starts_with("http/1.1 200"), "{text}");
    assert!(lower.contains("content-type: text/event-stream"), "{text}");
    assert!(lower.contains("transfer-encoding: chunked"), "{text}");
    assert!(count(&text, "data: ") >= 2, "only {} chunks: {text}", count(&text, "data: "));
    // And the body is still open: no last-chunk marker has been written yet.
    assert!(!text.contains("\r\n0\r\n\r\n"), "the body was finished early: {text}");
}

#[test]
fn the_connection_keeps_working_after_a_streamed_body_ends() {
    let server = start(SSE);
    let mut client = Client::to(&server);
    client.send("GET /once HTTP/1.1\r\nHost: t\r\n\r\n");
    let text = client.read_until("data: two", Duration::from_secs(5));
    assert!(text.contains("data: one"), "{text}");

    // The chunked body has ended, so this connection is back in the keep-alive loop.
    client.take();
    client.send("GET /stats HTTP/1.1\r\nHost: t\r\n\r\n");
    let (head, body) = client.response();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(body.contains("\"aborts\":0"), "{body}");
}

#[test]
fn a_client_that_leaves_stops_the_handler_timer() {
    let server = start(SSE);

    {
        let mut client = Client::to(&server);
        client.send("GET /events HTTP/1.1\r\nHost: t\r\n\r\n");
        let text = client.read_until("data: 2", Duration::from_secs(5));
        assert!(count(&text, "data: ") >= 2, "{text}");
        // Dropping the socket is what a browser does when the tab closes.
    }

    // Give the server the next chunk to fail on, which is how it learns the client is gone.
    let mut probe = Client::to(&server);
    let mut stats = String::new();
    for _ in 0..40 {
        probe.send("GET /stats HTTP/1.1\r\nHost: t\r\n\r\n");
        let (_, body) = probe.response();
        let fired = body.contains("\"aborts\":1");
        stats = body;
        if fired {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(stats.contains("\"aborts\":1"), "the abort never fired: {stats}");

    // The interval was cleared, so the tick count stops moving.
    let stopped_at = tick_count(&stats);
    std::thread::sleep(Duration::from_millis(60));
    probe.send("GET /stats HTTP/1.1\r\nHost: t\r\n\r\n");
    let (_, later) = probe.response();
    assert_eq!(tick_count(&later), stopped_at, "the timer kept running: {later}");
}

/// Pull ticks out of the stats body without pulling in a json parser.
fn tick_count(stats: &str) -> u32 {
    let start = stats.find("\"ticks\":").expect("ticks in the stats body") + 8;
    stats[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .expect("a number")
}
