use steady_state::*;

// over designed this enum is. much to learn here we have.
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
#[repr(u64)] // Pack everything into 8 bytes
pub(crate) enum FizzBuzzMessage {
    #[default]
    FizzBuzz = 15,         // Discriminant is 15 - could have been any valid FizzBuzz
    Fizz = 3,              // Discriminant is 3 - could have been any valid Fizz
    Buzz = 5,              // Discriminant is 5 - could have been any valid Buzz
    Value(u64),            // Store u64 directly, use the fact that FizzBuzz/Fizz/Buzz only occupy small values
}

impl FizzBuzzMessage {
    pub fn new(value: u64) -> Self {
        match (value % 3, value % 5) {
            (0, 0) => FizzBuzzMessage::FizzBuzz,    // Multiple of 15
            (0, _) => FizzBuzzMessage::Fizz,        // Multiple of 3, not 5
            (_, 0) => FizzBuzzMessage::Buzz,        // Multiple of 5, not 3
            _      => FizzBuzzMessage::Value(value), // Neither
        }
    }
}

pub(crate) struct WorkerState {
    pub(crate) heartbeats_processed: u64,
    pub(crate) values_processed: u64,
    pub(crate) messages_sent: u64,
    pub(crate) restart_count: u64,
}

pub async fn run(context: SteadyContext
                 , heartbeat: SteadyRx<u64>
                 , generator: SteadyRx<u64>
                 , logger: SteadyTx<FizzBuzzMessage>
                 , state: SteadyState<WorkerState>) -> Result<(),Box<dyn Error>> {
    let cmd = context.into_monitor([&heartbeat, &generator], [&logger]);
    if cmd.use_internal_behavior {
        internal_behavior(cmd, heartbeat, generator, logger, state).await
    } else {
        cmd.simulated_behavior(vec!(&logger)).await
    }
}

async fn internal_behavior<C: SteadyCommander>(mut cmd: C
                                               , heartbeat: SteadyRx<u64>
                                               , generator: SteadyRx<u64>
                                               , logger: SteadyTx<FizzBuzzMessage>
                                               , state: SteadyState<WorkerState>) -> Result<(),Box<dyn Error>> {

    let mut state = state.lock(|| WorkerState {
        heartbeats_processed: 0,
        values_processed: 0,
        messages_sent: 0,
        restart_count: 0,
    }).await;

    state.restart_count += 1;
    info!("Worker starting (restart #{}) with heartbeats: {}, values: {}, messages: {}", 
          state.restart_count, state.heartbeats_processed, state.values_processed, state.messages_sent);

    let mut heartbeat = heartbeat.lock().await;
    let mut generator = generator.lock().await;
    let mut logger = logger.lock().await;

    while cmd.is_running(|| i!(heartbeat.is_closed_and_empty()) && i!(generator.is_closed_and_empty()) && i!(logger.mark_closed())) {
        // Wait for both inputs to have data
        await_for_all!(cmd.wait_avail(&mut heartbeat, 1),
                       cmd.wait_avail(&mut generator, 1));

        // ROBUSTNESS DEMONSTRATION: Intentional panic for testing
        // NOTE: Future engineers should NEVER include panic conditions like this in production code!
        // This is only for demonstrating the framework's recovery capabilities.
        #[cfg(not(test))]  //do not throw in tests only at runtime.
        if state.heartbeats_processed == 5 && state.restart_count == 1 {
            error!("Worker intentionally panicking after {} heartbeats to demonstrate robustness!", state.heartbeats_processed);
            panic!("Intentional panic for robustness demonstration - DO NOT COPY THIS PATTERN!");
        }
        // END ROBUSTNESS DEMONSTRATION

        // Robust pattern: Peek at heartbeat, don't consume until we process everything
        if let Some(heartbeat_value) = cmd.try_peek(&mut heartbeat) {
            let heartbeat_val = *heartbeat_value;

            // Process all available generator values for this heartbeat
            let mut processed_values = Vec::new();

            // Peek at all available generator values
            for generator_value in cmd.try_peek_iter(&mut generator) {
                processed_values.push(*generator_value);
            }

            // Now process each value and send to logger
            let mut successfully_sent = 0;
            for (idx, value) in processed_values.iter().enumerate() {
                let fizz_buzz_msg = FizzBuzzMessage::new(*value);

                // Wait for room in logger channel
                await_for_all!(cmd.wait_vacant(&mut logger, 1));

                match cmd.try_send(&mut logger, fizz_buzz_msg) {
                    SendOutcome::Success => {
                        successfully_sent += 1;
                        state.messages_sent += 1;
                        trace!("Worker sent FizzBuzz message for value: {} -> {:?}", value, fizz_buzz_msg);
                    }
                    SendOutcome::Blocked(_) => {
                        // If we can't send, break and try again later
                        warn!("Worker logger channel blocked, will retry");
                        break;
                    }
                }
            }

            // Only advance generator position for successfully processed messages
            if successfully_sent > 0 {
                let advanced = cmd.advance_read_index(&mut generator, successfully_sent);
                state.values_processed += advanced as u64;
                trace!("Worker advanced generator by {} positions", advanced);
            }

            // If we processed all available generator values, consume the heartbeat
            if successfully_sent == processed_values.len() && !processed_values.is_empty() {
                if let Some(_) = cmd.try_take(&mut heartbeat) {
                    state.heartbeats_processed += 1;
                    trace!("Worker processed heartbeat: {}, total: {}", heartbeat_val, state.heartbeats_processed);
                }
            }
        }
    }

    info!("Worker shutting down. Heartbeats: {}, Values: {}, Messages: {}", 
          state.heartbeats_processed, state.values_processed, state.messages_sent);
    Ok(())
}

#[cfg(test)]
pub(crate) mod worker_tests {
    use std::thread::sleep;
    use steady_state::*;
    use super::*;

    #[test]
    fn test_worker() -> Result<(), Box<dyn Error>> {
        let mut graph = GraphBuilder::for_testing().build(());
        let (generate_tx, generate_rx) = graph.channel_builder().build();
        let (heartbeat_tx, heartbeat_rx) = graph.channel_builder().build();
        let (logger_tx, logger_rx) = graph.channel_builder().build::<FizzBuzzMessage>();

        let state = new_state();
        graph.actor_builder().with_name("UnitTest")
            .build(move |context| internal_behavior(context
                                                    , heartbeat_rx.clone()
                                                    , generate_rx.clone()
                                                    , logger_tx.clone()
                                                    , state.clone())
                   , &mut Threading::Spawn
            );

        generate_tx.testing_send_all(vec![0,1,2,3,4,5], true);
        heartbeat_tx.testing_send_all(vec![0], true);
        graph.start();

        sleep(Duration::from_millis(100));

        graph.request_stop();
        graph.block_until_stopped(Duration::from_secs(1))?;
        assert_steady_rx_eq_take!(&logger_rx, [FizzBuzzMessage::FizzBuzz
                                              ,FizzBuzzMessage::Value(1)
                                              ,FizzBuzzMessage::Value(2)
                                              ,FizzBuzzMessage::Fizz
                                              ,FizzBuzzMessage::Value(4)
                                              ,FizzBuzzMessage::Buzz]);
        Ok(())
    }
}
