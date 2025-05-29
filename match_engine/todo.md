- [] Use thiserror instead of anyhow (for each lib)
- [] Add event processing task
 - [] Store metadata (the symbol, and so on)
 - [] Store resent events in memory
 - [] Store cancelled events in memory
 - [] Can create a data damp to be stored in database
 - [] Process most requests about order book state
- [] Use crossbeam channel to communicate between engine and event processor task
- [] Generate order id using uuid crate
- [] Work on graceful shutdown to prevent data loss


Work on UUID generation for the orders !!!!!
