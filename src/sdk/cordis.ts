/**
 * Cordis Microkernel Core
 * Based on "A Programming Paradigm for Spatiotemporal Composability"
 * (Peking University & DeepSeek-AI)
 *
 * Implements:
 * 1. Temporal Composability (Revertible Effects & Fork Hierarchy with strict teardown)
 * 2. Spatial Composability (Reactive Coeffects, Scoped Contexts & Dynamic Service Injection)
 */

export type Disposable = () => void | Promise<void>;

export interface ServiceDescriptor<T = any> {
  name: string;
  instance: T;
}

export type PluginCallback<C = any> = (
  ctx: Context,
  config?: C
) => Disposable | void | Promise<Disposable | void>;

export interface PluginObject<C = any> {
  name: string;
  using?: string[];
  apply: PluginCallback<C>;
}

export type Plugin<C = any> = PluginCallback<C> | PluginObject<C>;

export type ForkStatus = "pending" | "active" | "suspended" | "disposed";

export class Fork {
  public status: ForkStatus = "pending";
  private disposables: Disposable[] = [];
  public childContext: Context;

  constructor(
    public readonly parent: Context,
    public readonly plugin: Plugin,
    public readonly config?: any
  ) {
    this.childContext = parent.extend();
  }

  public async start(): Promise<void> {
    if (this.status === "active" || this.status === "disposed") return;

    // Check if dependencies are satisfied (Spatial Composability)
    const required = this.getDependencies();
    const missing = required.filter((dep) => !this.parent.has(dep));

    if (missing.length > 0) {
      this.status = "suspended";
      return;
    }

    this.status = "active";

    try {
      let cleanup: Disposable | void | undefined;
      if (typeof this.plugin === "function") {
        cleanup = await this.plugin(this.childContext, this.config);
      } else if (typeof this.plugin.apply === "function") {
        cleanup = await this.plugin.apply(this.childContext, this.config);
      }

      if (typeof cleanup === "function") {
        this.disposables.push(cleanup);
      }
    } catch (err) {
      console.error(`[Cordis] Error mounting plugin:`, err);
      this.status = "suspended";
    }
  }

  public async dispose(): Promise<void> {
    if (this.status === "disposed") return;
    this.status = "disposed";

    // Dispose all child effects in reverse order (Temporal Composability)
    while (this.disposables.length > 0) {
      const fn = this.disposables.pop();
      if (fn) {
        try {
          await fn();
        } catch (e) {
          console.error(`[Cordis] Error in plugin disposable:`, e);
        }
      }
    }

    await this.childContext.dispose();
  }

  public getDependencies(): string[] {
    if (typeof this.plugin === "object" && Array.isArray(this.plugin.using)) {
      return this.plugin.using;
    }
    return [];
  }

  public getPluginName(): string {
    if (typeof this.plugin === "object" && this.plugin.name) {
      return this.plugin.name;
    }
    if (typeof this.plugin === "function" && this.plugin.name) {
      return this.plugin.name;
    }
    return "anonymous_plugin";
  }
}

export class Context {
  private services = new Map<string, any>();
  private forks: Fork[] = [];
  private disposables: Disposable[] = [];
  private eventListeners = new Map<string, Set<Function>>();

  constructor(public readonly parent?: Context) {}

  /** Create a child context inheriting parent services and event bus */
  public extend(): Context {
    return new Context(this);
  }

  /** Mount a plugin into this context (Spatiotemporal Fork) */
  public plugin<C = any>(plugin: Plugin<C>, config?: C): Fork {
    const fork = new Fork(this, plugin, config);
    this.forks.push(fork);
    void fork.start();
    return fork;
  }

  /** Register a service available to this context and children */
  public provide<T = any>(name: string, service: T): Disposable {
    this.services.set(name, service);
    this.emit(`service:${name}`, service);

    // Notify any suspended forks that may be waiting for this service
    this.reconcile();

    const cleanup = () => {
      if (this.services.get(name) === service) {
        this.services.delete(name);
        this.emit(`service-lost:${name}`, service);
        this.reconcile();
      }
    };

    this.disposables.push(cleanup);
    return cleanup;
  }

  /** Get a provided service from this context or parent hierarchy */
  public get<T = any>(name: string): T | undefined {
    if (this.services.has(name)) {
      return this.services.get(name) as T;
    }
    if (this.parent) {
      return this.parent.get<T>(name);
    }
    return undefined;
  }

  /** Check if a service is currently provided */
  public has(name: string): boolean {
    return this.get(name) !== undefined;
  }

  /** Reactively execute an effect when all dependencies become satisfied */
  public inject(deps: string[], callback: () => Disposable | void): Disposable {
    let effectCleanup: Disposable | void;

    const check = () => {
      const allPresent = deps.every((d) => this.has(d));
      if (allPresent && !effectCleanup) {
        effectCleanup = callback();
      } else if (!allPresent && effectCleanup) {
        if (typeof effectCleanup === "function") {
          try {
            effectCleanup();
          } catch (e) {
            console.error("[Cordis] Error in inject cleanup:", e);
          }
        }
        effectCleanup = undefined;
      }
    };

    // Listen to service changes
    const unsubscribers = deps.map((d) => {
      const u1 = this.on(`service:${d}`, check);
      const u2 = this.on(`service-lost:${d}`, check);
      return () => {
        u1();
        u2();
      };
    });

    check();

    const masterDisposable = () => {
      unsubscribers.forEach((u) => u());
      if (typeof effectCleanup === "function") {
        effectCleanup();
      }
    };

    this.disposables.push(masterDisposable);
    return masterDisposable;
  }

  /** Register an event listener */
  public on(event: string, handler: Function): Disposable {
    if (!this.eventListeners.has(event)) {
      this.eventListeners.set(event, new Set());
    }
    this.eventListeners.get(event)!.add(handler);

    const cleanup = () => {
      this.eventListeners.get(event)?.delete(handler);
    };
    this.disposables.push(cleanup);
    return cleanup;
  }

  /** Emit an event to this context and bubble up to parent */
  public emit(event: string, ...args: any[]): void {
    const handlers = this.eventListeners.get(event);
    if (handlers) {
      handlers.forEach((h) => {
        try {
          h(...args);
        } catch (e) {
          console.error(`[Cordis] Error in event '${event}':`, e);
        }
      });
    }
    if (this.parent) {
      this.parent.emit(event, ...args);
    }
  }

  /** Reconcile suspended forks against available services */
  public reconcile(): void {
    for (const fork of this.forks) {
      if (fork.status === "suspended") {
        const required = fork.getDependencies();
        if (required.every((d) => this.has(d))) {
          void fork.start();
        }
      } else if (fork.status === "active") {
        const required = fork.getDependencies();
        if (required.some((d) => !this.has(d))) {
          void fork.dispose();
          fork.status = "suspended";
        }
      }
    }
  }

  /** Teardown this context and all child forks */
  public async dispose(): Promise<void> {
    for (const fork of this.forks) {
      await fork.dispose();
    }
    this.forks = [];

    while (this.disposables.length > 0) {
      const fn = this.disposables.pop();
      if (fn) {
        try {
          await fn();
        } catch (e) {
          console.error("[Cordis] Error disposing context effect:", e);
        }
      }
    }

    this.services.clear();
    this.eventListeners.clear();
  }
}

/** Global root microkernel instance */
export const rootContext = new Context();
