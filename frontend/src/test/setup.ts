import "@testing-library/jest-dom/vitest";
import { beforeEach } from "vitest";
import { __resetHostOsCacheForTesting } from "../shared/os/useHostOs";

// jsdom lacks ResizeObserver; the Inspector's chart-sizing hook needs it.
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= ResizeObserverStub as unknown as typeof ResizeObserver;

// The `useHostOs` module caches the host OS in module scope for perf (see
// `shared/os/useHostOs.ts`). Reset that cache before every test so parallel
// test files don't leak a stale value — otherwise a test that first triggers
// the cache with a Tauri mock returning null/"unknown" would poison a later
// test that expects a fresh probe.
beforeEach(() => {
  __resetHostOsCacheForTesting();
});
