use steady_state::*;

/// FizzBuzzMessage is a compact enum for FizzBuzz logic.
/// The #[repr(u64)] ensures all variants fit in 8 bytes for efficient channel transport.
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
#[repr(u64)]
pub(crate) enum FizzBuzzMessage {
    #[default]
    FizzBuzz = 15,         // Discriminant is 15 - for multiples of 15
    Fizz = 3,              // Discriminant is 3 - for multiples of 3 (not 5)
    Buzz = 5,              // Discriminant is 5 - for multiples of 5 (not 3)
    Value(u64),            // For all other values
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

/// WorkerState holds persistent state for the Worker actor.
/// All fields are preserved across panics and restarts, ensuring
/// that no data is lost and the worker can resume exactly where it left off.
pub(crate) struct WorkerState {
    pub(crate) heartbeats_processed: u64,
    pub(crate) values_processed: u64,
    pub(crate) messages_sent: u64,
    pub(crate) restart_count: u64,
}

/// Entry point for the Worker actor.
/// Demonstrates robust, persistent state, peek-before-commit, and automatic restart.
pub async fn run(
    actor: SteadyActorShadow,
    heartbeat: SteadyRx<u64>,
    generator: SteadyRx<u64>,
    logger: SteadyTx<FizzBuzzMessage>,
    state: SteadyState<WorkerState>,
) -> Result<(), Box<dyn Error>> {
    internal_behavior(
        actor.into_spotlight([&heartbeat, &generator], [&logger]),
        heartbeat,
        generator,
        logger,
        state,
    )
        .await
}

/// Internal behavior for the Worker actor.
/// Demonstrates robust message processing, showstopper detection, and intentional failure injection.
/// The peek-before-commit pattern ensures that no message is lost or duplicated, even across panics.
async fn internal_behavior<A: SteadyActor>(
    mut actor: A,
    heartbeat: SteadyRx<u64>,
    generator: SteadyRx<u64>,
    logger: SteadyTx<FizzBuzzMessage>,
    state: SteadyState<WorkerState>,
) -> Result<(), Box<dyn Error>> {
    let mut state = state.lock(|| WorkerState {
        heartbeats_processed: 0,
        values_processed: 0,
        messages_sent: 0,
        restart_count: 0,
    }).await;

    state.restart_count += 1;
    info!(
        "Worker starting (restart #{}) with heartbeats: {}, values: {}, messages: {}",
        state.restart_count, state.heartbeats_processed, state.values_processed, state.messages_sent
    );

    let mut heartbeat = heartbeat.lock().await;
    let mut generator = generator.lock().await;
    let mut logger = logger.lock().await;

    while actor.is_running(|| {
        i!(heartbeat.is_closed_and_empty())
            && i!(generator.is_closed_and_empty())
            && i!(logger.mark_closed())
    }) {
        // Wait for both inputs to have data and logger to have space
        let clean = await_for_all!(
            actor.wait_avail(&mut heartbeat, 1),
            actor.wait_avail(&mut generator, 1),
            actor.wait_vacant(&mut logger, 1)
        );

        if clean {
            // Showstopper detection: if this value has been peeked N times, drop it and log.
            const SHOWSTOPPER_THRESHOLD: usize = 7;
            if actor.is_showstopper(&mut heartbeat, SHOWSTOPPER_THRESHOLD) {
                if let Some(value) = actor.try_take(&mut heartbeat) {
                    warn!(
                            "Showstopper detected: value {} has blocked the worker {} times, dropping it.",
                            value, SHOWSTOPPER_THRESHOLD
                        );
                    state.values_processed += 1;
                    continue; // Skip processing, go to the next iteration
                } else {
                    panic!("Showstopper detected, but heartbeat is empty!");
                }

            }

        }

        // --- Robustness Demonstration: Intentional Panic ---
        // This panic is injected to demonstrate automatic actor restart and state preservation.
        #[cfg(not(test))]
        if state.heartbeats_processed == 5 && state.restart_count == 1 {
            error!(
                "Worker intentionally panicking after {} heartbeats to demonstrate robustness!",
                state.heartbeats_processed
            );
            panic!("Intentional panic for robustness demonstration - DO NOT COPY THIS PATTERN!");
        }
        // --- End Robustness Demonstration ---

        // Only proceed if we have a heartbeat or if not all conditions were met (to avoid starvation)
        if actor.try_take(&mut heartbeat).is_some() || !clean {
            // Peek at the next generator value (do not take yet)
            if let Some(&value) = actor.try_peek(&mut generator) {


                // Process the value and send to logger
                let fizz_buzz_msg = FizzBuzzMessage::new(value);
                match actor.try_send(&mut logger, fizz_buzz_msg) {
                    SendOutcome::Success => {
                        // Only now do we take the value from the generator
                        let _ = actor.try_take(&mut generator);
                        state.values_processed += 1;
                        state.messages_sent += 1;
                        trace!(
                            "Worker sent FizzBuzz message for value: {} -> {:?}",
                            value,
                            fizz_buzz_msg
                        );
                    }
                    SendOutcome::Blocked(_) => {
                        // If we can't send, try again later
                        warn!("Worker logger channel blocked, will retry");
                        // Do not take the value, so we will try again next loop
                        continue;
                    }
                }
            }

            // Always advance heartbeat count if we processed a value or dropped a showstopper
            state.heartbeats_processed += 1;
            trace!(
                "Worker processed heartbeat total: {}",
                state.heartbeats_processed
            );
        }
    }

    info!(
        "Worker shutting down. Heartbeats: {}, Values: {}, Messages: {}",
        state.heartbeats_processed, state.values_processed, state.messages_sent
    );
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
                   , SoloAct
            );

        generate_tx.testing_send_all(vec![0,1,2,3,4,5], true);
        heartbeat_tx.testing_send_all(vec![0], true);
        graph.start();

        sleep(Duration::from_millis(100));

        graph.request_shutdown();
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
