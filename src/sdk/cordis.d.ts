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
export type PluginCallback<C = any> = (ctx: Context, config?: C) => Disposable | void | Promise<Disposable | void>;
export interface PluginObject<C = any> {
    name: string;
    using?: string[];
    apply: PluginCallback<C>;
}
export type Plugin<C = any> = PluginCallback<C> | PluginObject<C>;
export type ForkStatus = "pending" | "active" | "suspended" | "disposed";
export declare class Fork {
    readonly parent: Context;
    readonly plugin: Plugin;
    readonly config?: any | undefined;
    status: ForkStatus;
    private disposables;
    childContext: Context;
    constructor(parent: Context, plugin: Plugin, config?: any | undefined);
    start(): Promise<void>;
    dispose(): Promise<void>;
    getDependencies(): string[];
    getPluginName(): string;
}
export declare class Context {
    readonly parent?: Context | undefined;
    private services;
    private forks;
    private disposables;
    private eventListeners;
    constructor(parent?: Context | undefined);
    /** Create a child context inheriting parent services and event bus */
    extend(): Context;
    /** Mount a plugin into this context (Spatiotemporal Fork) */
    plugin<C = any>(plugin: Plugin<C>, config?: C): Fork;
    /** Register a service available to this context and children */
    provide<T = any>(name: string, service: T): Disposable;
    /** Get a provided service from this context or parent hierarchy */
    get<T = any>(name: string): T | undefined;
    /** Check if a service is currently provided */
    has(name: string): boolean;
    /** Reactively execute an effect when all dependencies become satisfied */
    inject(deps: string[], callback: () => Disposable | void): Disposable;
    /** Register an event listener */
    on(event: string, handler: Function): Disposable;
    /** Emit an event to this context and bubble up to parent */
    emit(event: string, ...args: any[]): void;
    /** Reconcile suspended forks against available services */
    reconcile(): void;
    /** Teardown this context and all child forks */
    dispose(): Promise<void>;
}
/** Global root microkernel instance */
export declare const rootContext: Context;
//# sourceMappingURL=cordis.d.ts.map