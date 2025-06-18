use std::thread::sleep;
use steady_state::*;
use crate::actor::worker::FizzBuzzMessage;

/// LoggerState holds persistent state for the Logger actor.
/// All fields are preserved across panics and restarts, ensuring
/// that no data is lost and the logger can resume exactly where it left off.
pub(crate) struct LoggerState {
    pub(crate) messages_logged: u64,
    pub(crate) fizz_count: u64,
    pub(crate) buzz_count: u64,
    pub(crate) fizzbuzz_count: u64,
    pub(crate) value_count: u64,
    pub(crate) restart_count: u64,
}

/// Entry point for the Logger actor.
/// Demonstrates robust, persistent state, peek-before-commit, and automatic restart.
pub async fn run(
    actor: SteadyActorShadow,
    fizz_buzz_rx: SteadyRx<FizzBuzzMessage>,
    state: SteadyState<LoggerState>,
) -> Result<(), Box<dyn Error>> {
    let actor = actor.into_spotlight([&fizz_buzz_rx], []);
    if actor.use_internal_behavior {
        internal_behavior(actor, fizz_buzz_rx, state).await
    } else {
        actor.simulated_behavior(vec!(&fizz_buzz_rx)).await
    }
}

/// Internal behavior for the Logger actor.
/// Demonstrates robust message processing, showstopper detection, and intentional failure injection.
/// The peek-before-commit pattern ensures that no message is lost or duplicated, even across panics.
async fn internal_behavior<A: SteadyActor>(
    mut actor: A,
    rx: SteadyRx<FizzBuzzMessage>,
    state: SteadyState<LoggerState>,
) -> Result<(), Box<dyn Error>> {
    let mut state = state.lock(|| LoggerState {
        messages_logged: 0,
        fizz_count: 0,
        buzz_count: 0,
        fizzbuzz_count: 0,
        value_count: 0,
        restart_count: 0,
    }).await;

    state.restart_count += 1;
    info!(
        "Logger starting (restart #{}) with {} messages logged (F:{}, B:{}, FB:{}, V:{})",
        state.restart_count, state.messages_logged, state.fizz_count, state.buzz_count,
        state.fizzbuzz_count, state.value_count
    );

    let mut rx = rx.lock().await;

    while actor.is_running(|| rx.is_closed_and_empty()) {
        await_for_all!(actor.wait_avail(&mut rx, 1));

        // --- Robustness Demonstration: Intentional Panic ---
        #[cfg(not(test))]
        if state.messages_logged == 3 && state.restart_count == 1 {
            error!(
                "Logger intentionally panicking after {} messages to demonstrate robustness!",
                state.messages_logged
            );
            panic!("Intentional panic for robustness demonstration - DO NOT COPY THIS PATTERN!");
        }
        // --- End Robustness Demonstration ---

        // Showstopper detection: if this message has been peeked N times, drop it and log.
        if actor.is_showstopper(&mut rx, 7) {
            // This same peeked message caused us to panic 7 times in a row, so we drop it.
            actor.try_take(&mut rx).expect("internal error");
            continue; // Back to top of loop
        }

        // Peek-before-commit: Only after successful processing do we advance the read position.
        if let Some(peeked_msg) = actor.try_peek(&mut rx) {
            let msg = *peeked_msg;

            // Process the message (this is our "work" that we don't want to lose)
            match msg {
                FizzBuzzMessage::Fizz => {
                    state.fizz_count += 1;
                    info!("Msg {:?} (Fizz total: {})", msg, state.fizz_count);
                }
                FizzBuzzMessage::Buzz => {
                    state.buzz_count += 1;
                    info!("Msg {:?} (Buzz total: {})", msg, state.buzz_count);
                }
                FizzBuzzMessage::FizzBuzz => {
                    state.fizzbuzz_count += 1;
                    info!("Msg {:?} (FizzBuzz total: {})", msg, state.fizzbuzz_count);
                }
                FizzBuzzMessage::Value(v) => {
                    state.value_count += 1;
                    info!("Msg {:?} (Value total: {})", msg, state.value_count);
                }
            }

            // Only after successful processing do we advance the read position
            let advanced = actor.advance_read_index(&mut rx, 1).item_count();
            if advanced > 0 {
                state.messages_logged += 1;
                trace!(
                    "Logger advanced read position, total messages: {}",
                    state.messages_logged
                );
            }
        }
    }

    info!(
        "Logger shutting down. Total: {} (F:{}, B:{}, FB:{}, V:{})",
        state.messages_logged, state.fizz_count, state.buzz_count,
        state.fizzbuzz_count, state.value_count
    );
    Ok(())
}

#[test]
fn test_logger() -> Result<(), Box<dyn std::error::Error>> {
    use steady_logger::*;
    let _guard = start_log_capture();

    let mut graph = GraphBuilder::for_testing().build(());
    let (fizz_buzz_tx, fizz_buzz_rx) = graph.channel_builder().build();

    let state = new_state();
    graph.actor_builder().with_name("UnitTest")
        .build(move |context| {
            internal_behavior(context, fizz_buzz_rx.clone(), state.clone())
        }
               , SoloAct);

    graph.start();
    fizz_buzz_tx.testing_send_all(vec![FizzBuzzMessage::Fizz],true);
    sleep(Duration::from_millis(300));
    graph.request_shutdown();
    graph.block_until_stopped(Duration::from_secs(10000))?;
    assert_in_logs!(["Msg Fizz"]);

    Ok(())
}
