import { definePlugin, type PluginDefinition } from "./sdk";
import { AnalyticsPage } from "./AnalyticsPage";
import manifest from "../plugin.json";

export const analyticsPlugin: PluginDefinition = definePlugin({
  manifest: manifest as any,
  renderView: AnalyticsPage,
  apply: () => {
    console.log(`[Plugin Harness] Analytics & Telemetry plugin mounted`);
    return () => {
      console.log(`[Plugin Harness] Analytics & Telemetry plugin unmounted`);
    };
  },
});

export default analyticsPlugin;
export { AnalyticsPage };

