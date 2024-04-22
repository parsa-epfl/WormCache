This component defines the reference cache model used in QFlex. 
It includes the following components:
- Two private caches, with L1i and L1d.
- A directory, for two private caches. 
- A non-inclusive shared L2 cache.

The private cache should be pretty clear in terms of implementation. Each cache line has three possible states:
- Exclusive: the cache line contains a clean and writable data.
- Valid: The cache line contains a valid copy of the data.
- Writable: The cache line is writable.
- Dirty: The cache line has dirty data.

TODO: Merge this module together with the parallel cache.