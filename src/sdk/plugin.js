/** Helper to define a typed xConsole plugin with intellisense */
export function definePlugin(definition) {
    return definition;
}
/** Convert a PluginDefinition into a Cordis PluginObject */
export function toCordisPlugin(definition, pluginCtx) {
    return {
        name: definition.manifest.id,
        using: definition.using || definition.manifest.using || [],
        apply: async (ctx) => {
            const cleanups = [];
            if (definition.onMount) {
                await definition.onMount(pluginCtx);
            }
            if (definition.apply) {
                const res = await definition.apply(ctx, pluginCtx);
                if (typeof res === "function") {
                    cleanups.push(res);
                }
            }
            return async () => {
                while (cleanups.length > 0) {
                    const fn = cleanups.pop();
                    if (fn)
                        await fn();
                }
                if (definition.onUnmount) {
                    await definition.onUnmount(pluginCtx);
                }
            };
        },
    };
}
//# sourceMappingURL=plugin.js.map