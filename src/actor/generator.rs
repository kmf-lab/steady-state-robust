use steady_state::*;

/// GeneratorState holds all persistent state for the Generator actor.
/// All fields are preserved across actor panics and restarts, ensuring
/// that no data is lost and the generator can resume exactly where it left off.
pub(crate) struct GeneratorState {
    /// The next value to generate and send.
    pub(crate) value: u64,
    /// The total number of messages sent so far.
    pub(crate) messages_sent: u64,
    /// Counter for intentional panics (for robustness demonstration).
    pub(crate) panic_counter: u64,
}

/// Entry point for the Generator actor.
/// This actor demonstrates robust, persistent state and automatic restart.
pub async fn run(
    actor: SteadyActorShadow,
    generated_tx: SteadyTx<u64>,
    state: SteadyState<GeneratorState>,
) -> Result<(), Box<dyn Error>> {
    let actor = actor.into_spotlight([], [&generated_tx]);
    if actor.use_internal_behavior {
        internal_behavior(actor, generated_tx, state).await
    } else {
        actor.simulated_behavior(vec!(&generated_tx)).await
    }
}

/// Internal behavior for the Generator actor.
/// Demonstrates the peek-before-commit pattern and intentional failure injection.
/// State is always updated only after a successful send, ensuring no duplicate or lost messages.
async fn internal_behavior<A: SteadyActor>(
    mut actor: A,
    generated: SteadyTx<u64>,
    state: SteadyState<GeneratorState>,
) -> Result<(), Box<dyn Error>> {
    // Lock the persistent state for this actor instance.
    let mut state = state.lock(|| GeneratorState {
        value: 0,
        messages_sent: 0,
        panic_counter: 0,
    }).await;
    let mut generated = generated.lock().await;

    info!(
        "Generator starting with value: {}, messages_sent: {}",
        state.value, state.messages_sent
    );

    while actor.is_running(|| generated.mark_closed()) {
        // Wait for room in the channel before attempting to send.
        await_for_all!(actor.wait_vacant(&mut generated, 1));

        // --- Robustness Demonstration: Intentional Panic ---
        // This panic is injected to demonstrate automatic actor restart and state preservation.
        // In production, replace with real error handling.
        state.panic_counter += 1;
        #[cfg(not(test))]
        if state.panic_counter == 13 {
            error!(
                "Generator intentionally panicking at message {} to demonstrate robustness!",
                state.value
            );
            panic!("Intentional panic for robustness demonstration - DO NOT COPY THIS PATTERN!");
        }
        // --- End Robustness Demonstration ---

        // Peek-before-commit: Only update state after a successful send.
        if !actor.is_full(&mut generated) {
            let message_to_send = state.value;

            // Attempt to send the message.
            match actor.try_send(&mut generated, message_to_send) {
                SendOutcome::Success => {
                    // Only after a successful send do we update state.
                    state.value += 1;
                    state.messages_sent += 1;
                    trace!(
                        "Generator sent: {}, total sent: {}",
                        message_to_send,
                        state.messages_sent
                    );
                }
                SendOutcome::Blocked(_) => {
                    // Channel became full, try again next loop.
                    continue;
                }
            }
        }
    }

    info!(
        "Generator shutting down. Final value: {}, total sent: {}",
        state.value, state.messages_sent
    );
    Ok(())
}

#[cfg(test)]
pub(crate) mod generator_tests {
    use std::thread::sleep;
    use steady_state::*;
    use super::*;

    #[test]
    fn test_generator() -> Result<(), Box<dyn Error>> {
        let mut graph = GraphBuilder::for_testing().build(());
        let (generate_tx, generate_rx) = graph.channel_builder().build();

        let state = new_state();
        graph.actor_builder()
            .with_name("UnitTest")
            .build(move |context| internal_behavior(context, generate_tx.clone(), state.clone()), SoloAct );

        graph.start();
        sleep(Duration::from_millis(100));
        graph.request_shutdown();

        graph.block_until_stopped(Duration::from_secs(1))?;

        assert_steady_rx_eq_take!(generate_rx,vec!(0,1));
        Ok(())
    }
}
