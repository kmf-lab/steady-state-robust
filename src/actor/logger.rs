use std::thread::sleep;
use steady_state::*;
use crate::actor::worker::FizzBuzzMessage;

pub(crate) struct LoggerState {
    pub(crate) messages_logged: u64,
    pub(crate) fizz_count: u64,
    pub(crate) buzz_count: u64,
    pub(crate) fizzbuzz_count: u64,
    pub(crate) value_count: u64,
    pub(crate) restart_count: u64,
}

pub async fn run(context: SteadyContext, fizz_buzz_rx: SteadyRx<FizzBuzzMessage>, state: SteadyState<LoggerState>) -> Result<(),Box<dyn Error>> {
    let cmd = context.into_monitor([&fizz_buzz_rx], []);
    if cmd.use_internal_behavior {
        internal_behavior(cmd, fizz_buzz_rx, state).await
    } else {
        cmd.simulated_behavior(vec!(&fizz_buzz_rx)).await
    }
}

async fn internal_behavior<C: SteadyCommander>(mut cmd: C, rx: SteadyRx<FizzBuzzMessage>, state: SteadyState<LoggerState>) -> Result<(),Box<dyn Error>> {
    let mut state = state.lock(|| LoggerState {
        messages_logged: 0,
        fizz_count: 0,
        buzz_count: 0,
        fizzbuzz_count: 0,
        value_count: 0,
        restart_count: 0,
    }).await;

    state.restart_count += 1;
    info!("Logger starting (restart #{}) with {} messages logged (F:{}, B:{}, FB:{}, V:{})", 
          state.restart_count, state.messages_logged, state.fizz_count, state.buzz_count, 
          state.fizzbuzz_count, state.value_count);

    let mut rx = rx.lock().await;

    while cmd.is_running(|| i!(rx.is_closed_and_empty())) {
        await_for_all!(cmd.wait_avail(&mut rx, 1));

        // ROBUSTNESS DEMONSTRATION: Intentional panic for testing
        // NOTE: Future engineers should NEVER include panic conditions like this in production code!
        // This is only for demonstrating the framework's recovery capabilities.
        #[cfg(not(test))]  //do not throw in tests only at runtime.
        if state.messages_logged == 3 && state.restart_count == 1 {
            error!("Logger intentionally panicking after {} messages to demonstrate robustness!", state.messages_logged);
            panic!("Intentional panic for robustness demonstration - DO NOT COPY THIS PATTERN!");
        }
        // END ROBUSTNESS DEMONSTRATION

        // Robust pattern: Peek at the message, process it, then advance
        if let Some(peeked_msg) = cmd.try_peek(&mut rx) {
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
            let advanced = cmd.advance_read_index(&mut rx, 1);
            if advanced > 0 {
                state.messages_logged += 1;
                trace!("Logger advanced read position, total messages: {}", state.messages_logged);
            }
        }
    }

    info!("Logger shutting down. Total: {} (F:{}, B:{}, FB:{}, V:{})", 
          state.messages_logged, state.fizz_count, state.buzz_count, 
          state.fizzbuzz_count, state.value_count);
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
               , &mut Threading::Spawn);

    graph.start();
    fizz_buzz_tx.testing_send_all(vec![FizzBuzzMessage::Fizz],true);
    sleep(Duration::from_millis(300));
    graph.request_stop();
    graph.block_until_stopped(Duration::from_secs(10000))?;
    assert_in_logs!(["Msg Fizz"]);

    Ok(())
}
