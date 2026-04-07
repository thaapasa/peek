/**
 * Type-safe event bus with wildcard support and async listeners.
 */

type EventMap = Record<string, unknown>;
type Listener<T = unknown> = (payload: T) => void | Promise<void>;

interface Subscription {
  unsubscribe(): void;
}

class EventBus<Events extends EventMap> {
  private listeners = new Map<keyof Events, Set<Listener>>();
  private wildcardListeners = new Set<Listener<{ event: string; payload: unknown }>>();

  on<K extends keyof Events>(event: K, listener: Listener<Events[K]>): Subscription {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set());
    }
    this.listeners.get(event)!.add(listener as Listener);

    return {
      unsubscribe: () => {
        this.listeners.get(event)?.delete(listener as Listener);
      },
    };
  }

  onAny(listener: Listener<{ event: string; payload: unknown }>): Subscription {
    this.wildcardListeners.add(listener);
    return {
      unsubscribe: () => this.wildcardListeners.delete(listener),
    };
  }

  async emit<K extends keyof Events>(event: K, payload: Events[K]): Promise<void> {
    const promises: Promise<void>[] = [];

    for (const listener of this.listeners.get(event) ?? []) {
      const result = listener(payload);
      if (result instanceof Promise) promises.push(result);
    }

    for (const listener of this.wildcardListeners) {
      const result = listener({ event: event as string, payload });
      if (result instanceof Promise) promises.push(result);
    }

    await Promise.all(promises);
  }

  once<K extends keyof Events>(event: K, listener: Listener<Events[K]>): Subscription {
    const sub = this.on(event, (payload) => {
      sub.unsubscribe();
      return listener(payload);
    });
    return sub;
  }

  clear(event?: keyof Events): void {
    if (event) {
      this.listeners.delete(event);
    } else {
      this.listeners.clear();
      this.wildcardListeners.clear();
    }
  }
}

// --- Usage ---

interface AppEvents {
  "user:login": { userId: string; timestamp: Date };
  "user:logout": { userId: string };
  "file:open": { path: string; size: number };
  "theme:change": { from: string; to: string };
}

const bus = new EventBus<AppEvents>();

bus.on("user:login", ({ userId, timestamp }) => {
  console.log(`User ${userId} logged in at ${timestamp.toISOString()}`);
});

bus.on("theme:change", async ({ from, to }) => {
  console.log(`Theme changing: ${from} → ${to}`);
  await new Promise((resolve) => setTimeout(resolve, 100));
  console.log("Theme transition complete");
});

bus.onAny(({ event, payload }) => {
  console.log(`[audit] ${event}:`, JSON.stringify(payload));
});

// Fire events
await bus.emit("user:login", { userId: "alice", timestamp: new Date() });
await bus.emit("theme:change", { from: "islands-dark", to: "vivid-dark" });
