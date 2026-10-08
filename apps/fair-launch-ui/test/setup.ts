import '../src/polyfills.ts';
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

// jsdom has its own Uint8Array, so a Node Buffer is not an instance of it and the ed25519 and hashing code
// behind PDA derivation rejects its inputs. Use Node's Uint8Array (the one Buffer extends) in the tests.
globalThis.Uint8Array = Object.getPrototypeOf(Buffer) as typeof Uint8Array;

afterEach(() => {
  cleanup();
  try {
    window.localStorage.clear();
  } catch {
    // storage may be unavailable
  }
});
