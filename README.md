# Steady State Robust

> **Lesson 4: Building Fault-Tolerant, Self-Healing Actor Systems**

This lesson demonstrates how to build actor systems that **recover automatically from failure**, **preserve all critical state**, and **guarantee message integrity**—all using the `steady_state` framework.  
It builds on the batching, performance, and memory safety lessons before it, and introduces the most important property of any real distributed system: **robustness**.

---

## 🛡️ What Makes This "Robust"?

**Robustness** means the system keeps working—even when things go wrong.  
In this lesson, you’ll see:

- **Automatic Actor Restart:** If an actor panics (crashes), it is restarted automatically, with all its important state preserved.
- **Persistent State:** Counters, statistics, and progress are never lost—even after repeated failures.
- **Peek-Before-Commit:** Messages are only removed from the channel after successful processing, so no message is lost or duplicated, even if the actor fails mid-task.
- **Failure Isolation:** One actor’s failure never brings down the whole system. Each actor is its own “failure domain.”
- **Recovery Tracking:** The system tracks and reports how many times each actor has restarted, so you can see resilience in action.

---

## 🏗️ System Architecture

**Resilient Pipeline:**

```
Generator → Worker → Logger
↗
Heartbeat
```

- **Generator:** Produces a sequence of numbers, simulates failures, and demonstrates state recovery.
- **Heartbeat:** Coordinates timing, and restarts cleanly after failure.
- **Worker:** Converts numbers to FizzBuzz, robustly peeks and commits messages, and demonstrates Dead Letter Queue (DLQ) handling for “showstopper” messages.
- **Logger:** Categorizes and logs messages, tracks statistics, and survives repeated failures.

---

## 🧠 Key Robustness Concepts

### What’s New in This Lesson?

- **Automatic Recovery:** The system restarts failed actors for you—no manual intervention required.
- **State That Survives Crashes:** All important counters and statistics are stored in a persistent state object, so actors pick up exactly where they left off.
- **Peek-Before-Commit:** Actors always peek at a message before processing. If they crash, the message is still there for the next run.
- **Showstopper Detection:** If a message causes repeated failures, the system can detect and drop it, preventing infinite crash loops.
- **Graceful Shutdown:** Even after multiple failures, the system can shut down cleanly, ensuring all work is finished or accounted for.

### Why Is This Important?

- **Real systems fail.** Hardware dies, code panics, and networks drop messages.  
  Robustness means your system keeps going, no matter what.
- **No data loss.** With peek-before-commit, you never lose a message—even if you crash in the middle of processing.
- **No duplicate work.** State is only updated after success, so you never process the same message twice.
- **No cascading failures.** One bad actor doesn’t take down the rest.

---

## 🧪 How Does It Work?

- **Persistent State:** Each actor’s state is stored in a special object that survives panics and restarts.
- **Automatic Restart:** The framework detects panics and restarts the actor, passing it its last state.
- **Peek-Before-Commit:** Actors use `peek` to look at a message, process it, and only then `take` (commit) it.
- **Showstopper Handling:** If a message is peeked (but not taken) too many times, it’s considered a “showstopper” and can be dropped or logged for investigation.
- **Restart Metrics:** Each actor tracks how many times it has restarted, so you can see resilience in action.

---

## 🏆 What Will You See?

- **Actors that crash and recover automatically.**
- **No lost or duplicated messages, even after repeated failures.**
- **State (counters, statistics) that continues seamlessly across restarts.**
- **Logs showing when actors restart, and when “showstopper” messages are detected and handled.**

---

## 🛠️ Try It Yourself

```bash
# Run with default robust settings (1s heartbeat, 60 beats)
cargo run

# Simulate more frequent failures
cargo run -- --rate 100 --beats 10

# Watch the logs for actor restarts, state recovery, and DLQ handling
RUST_LOG=info cargo run
```

---

## Takeaways

- **Robustness is not an afterthought—it’s a design principle.**
- **Peek-before-commit** is the gold standard for reliable message processing.
- **Persistent state** is the key to seamless recovery.
- **Automatic restart** and **failure isolation** make your system self-healing.
- **Metrics and tracking** let you see and trust your system’s resilience.

---

## ⚠️ Educational Purpose Notice

This lesson includes intentional panics and failures to demonstrate recovery.  
**Never use intentional panics in production!**  
Instead, use these patterns—persistent state, peek-before-commit, and automatic restart—to build real, robust systems.

---

## 🚦 Next Steps

- Try breaking the system in new ways—see how it recovers!
- Experiment with different failure rates and message patterns.
- Think about how you’d extend these patterns to distributed or cloud systems.
- Review the code and comments to see how each robustness feature is implemented.

---

## 📚 Further Reading

- [The Actor Model](https://en.wikipedia.org/wiki/Actor_model)
- [Designing Data-Intensive Applications](https://dataintensive.net/) (see chapters on fault tolerance and recovery)
- [The Reactive Manifesto](https://www.reactivemanifesto.org/)

