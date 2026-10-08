import { Buffer } from 'buffer';

// web3.js and the client use Buffer; browsers do not ship one.
(globalThis as unknown as { Buffer: typeof Buffer }).Buffer ??= Buffer;
