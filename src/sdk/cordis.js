/**
 * Cordis Microkernel Core
 * Based on "A Programming Paradigm for Spatiotemporal Composability"
 * (Peking University & DeepSeek-AI)
 *
 * Implements:
 * 1. Temporal Composability (Revertible Effects & Fork Hierarchy with strict teardown)
 * 2. Spatial Composability (Reactive Coeffects, Scoped Contexts & Dynamic Service Injection)
 */
export class Fork {
    parent;
    plugin;
    config;
    status = "pending";
    disposables = [];
    childContext;
    constructor(parent, plugin, config) {
        this.parent = parent;
        this.plugin = plugin;
        this.config = config;
        this.childContext = parent.extend();
    }
    async start() {
        if (this.status === "active" || this.status === "disposed")
            return;
        // Check if dependencies are satisfied (Spatial Composability)
        const required = this.getDependencies();
        const missing = required.filter((dep) => !this.parent.has(dep));
        if (missing.length > 0) {
            this.status = "suspended";
            return;
        }
        this.status = "active";
        try {
            let cleanup;
            if (typeof this.plugin === "function") {
                cleanup = await this.plugin(this.childContext, this.config);
            }
            else if (typeof this.plugin.apply === "function") {
                cleanup = await this.plugin.apply(this.childContext, this.config);
            }
            if (typeof cleanup === "function") {
                this.disposables.push(cleanup);
            }
        }
        catch (err) {
            console.error(`[Cordis] Error mounting plugin:`, err);
            this.status = "suspended";
        }
    }
    async dispose() {
        if (this.status === "disposed")
            return;
        this.status = "disposed";
        // Dispose all child effects in reverse order (Temporal Composability)
        while (this.disposables.length > 0) {
            const fn = this.disposables.pop();
            if (fn) {
                try {
                    await fn();
                }
                catch (e) {
                    console.error(`[Cordis] Error in plugin disposable:`, e);
                }
            }
        }
        await this.childContext.dispose();
    }
    getDependencies() {
        if (typeof this.plugin === "object" && Array.isArray(this.plugin.using)) {
            return this.plugin.using;
        }
        return [];
    }
    getPluginName() {
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
    parent;
    services = new Map();
    forks = [];
    disposables = [];
    eventListeners = new Map();
    constructor(parent) {
        this.parent = parent;
    }
    /** Create a child context inheriting parent services and event bus */
    extend() {
        return new Context(this);
    }
    /** Mount a plugin into this context (Spatiotemporal Fork) */
    plugin(plugin, config) {
        const fork = new Fork(this, plugin, config);
        this.forks.push(fork);
        void fork.start();
        return fork;
    }
    /** Register a service available to this context and children */
    provide(name, service) {
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
    get(name) {
        if (this.services.has(name)) {
            return this.services.get(name);
        }
        if (this.parent) {
            return this.parent.get(name);
        }
        return undefined;
    }
    /** Check if a service is currently provided */
    has(name) {
        return this.get(name) !== undefined;
    }
    /** Reactively execute an effect when all dependencies become satisfied */
    inject(deps, callback) {
        let effectCleanup;
        const check = () => {
            const allPresent = deps.every((d) => this.has(d));
            if (allPresent && !effectCleanup) {
                effectCleanup = callback();
            }
            else if (!allPresent && effectCleanup) {
                if (typeof effectCleanup === "function") {
                    try {
                        effectCleanup();
                    }
                    catch (e) {
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
    on(event, handler) {
        if (!this.eventListeners.has(event)) {
            this.eventListeners.set(event, new Set());
        }
        this.eventListeners.get(event).add(handler);
        const cleanup = () => {
            this.eventListeners.get(event)?.delete(handler);
        };
        this.disposables.push(cleanup);
        return cleanup;
    }
    /** Emit an event to this context and bubble up to parent */
    emit(event, ...args) {
        const handlers = this.eventListeners.get(event);
        if (handlers) {
            handlers.forEach((h) => {
                try {
                    h(...args);
                }
                catch (e) {
                    console.error(`[Cordis] Error in event '${event}':`, e);
                }
            });
        }
        if (this.parent) {
            this.parent.emit(event, ...args);
        }
    }
    /** Reconcile suspended forks against available services */
    reconcile() {
        for (const fork of this.forks) {
            if (fork.status === "suspended") {
                const required = fork.getDependencies();
                if (required.every((d) => this.has(d))) {
                    void fork.start();
                }
            }
            else if (fork.status === "active") {
                const required = fork.getDependencies();
                if (required.some((d) => !this.has(d))) {
                    void fork.dispose();
                    fork.status = "suspended";
                }
            }
        }
    }
    /** Teardown this context and all child forks */
    async dispose() {
        for (const fork of this.forks) {
            await fork.dispose();
        }
        this.forks = [];
        while (this.disposables.length > 0) {
            const fn = this.disposables.pop();
            if (fn) {
                try {
                    await fn();
                }
                catch (e) {
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
//# sourceMappingURL=cordis.js.map