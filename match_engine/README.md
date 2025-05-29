This is a mathcing engine lib.

Engine is synchronous
Engine handles all the information about the orders

The engine itself runs on a dedicated thread using tokio::spawn_blocking
Also, the complimentary task is running along side with the engine - this is async task, that stored the engine metadata.
This task is responsible for
- running the blocking task
- processing external communication
- sending events to the engine
- processing events from the engine
- storing resent engine data
- preparing data for database storing
