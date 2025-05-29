use steady_state::*;

pub(crate) struct GeneratorState {
    pub(crate) value: u64,
    pub(crate) messages_sent: u64,
    pub(crate) panic_counter: u64,  // For demonstrating robustness
}

pub async fn run(context: SteadyContext, generated_tx: SteadyTx<u64>, state: SteadyState<GeneratorState>) -> Result<(),Box<dyn Error>> {
    let cmd = context.into_monitor([], [&generated_tx]);
    if cmd.use_internal_behavior {
        internal_behavior(cmd, generated_tx, state).await
    } else {
        cmd.simulated_behavior(vec!(&generated_tx)).await
    }
}

async fn internal_behavior<C: SteadyCommander>(mut cmd: C, generated: SteadyTx<u64>, state: SteadyState<GeneratorState> ) -> Result<(),Box<dyn Error>> {

    let mut state = state.lock(|| GeneratorState {
        value: 0,
        messages_sent: 0,
        panic_counter: 0,
    }).await;
    let mut generated = generated.lock().await;

    info!("Generator starting with value: {}, messages_sent: {}", state.value, state.messages_sent);

    while cmd.is_running(|| i!(generated.mark_closed())) {
        // Wait for room in the channel
        await_for_all!(cmd.wait_vacant(&mut generated, 1));

        // ROBUSTNESS DEMONSTRATION: Intentional panic for testing
        // NOTE: Future engineers should NEVER include panic conditions like this in production code!
        // This is only for demonstrating the framework's recovery capabilities.
        state.panic_counter += 1;
        #[cfg(not(test))]  //do not throw in tests only at runtime.
        if state.panic_counter == 13 {
            error!("Generator intentionally panicking at message {} to demonstrate robustness!", state.value);
            panic!("Intentional panic for robustness demonstration - DO NOT COPY THIS PATTERN!");
        }
        // END ROBUSTNESS DEMONSTRATION

        // Robust pattern: Check we can send, then prepare the message, then send
        if !cmd.is_full(&mut generated) {
            let message_to_send = state.value;

            // Send the message
            match cmd.try_send(&mut generated, message_to_send) {
                SendOutcome::Success => {
                    state.value += 1;
                    state.messages_sent += 1;
                    trace!("Generator sent: {}, total sent: {}", message_to_send, state.messages_sent);
                }
                SendOutcome::Blocked(_) => {
                    // Channel became full between check and send, continue to next iteration
                    continue;
                }
            }
        }
    }

    info!("Generator shutting down. Final value: {}, total sent: {}", state.value, state.messages_sent);
    Ok(())
}

/// Here we test the internal behavior of this actor
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
            .build_spawn(move |context| internal_behavior(context, generate_tx.clone(), state.clone()) );

        graph.start();
        sleep(Duration::from_millis(100));
        graph.request_shutdown();

        graph.block_until_stopped(Duration::from_secs(1))?;

        assert_steady_rx_eq_take!(generate_rx,vec!(0,1));
        Ok(())
    }
}
