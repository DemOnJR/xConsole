import { lazy } from "react";
import { definePlugin, type PluginDefinition } from "./sdk";
import manifest from "../plugin.json";

const TeamsPage = lazy(() => import("./TeamsPage").then((m) => ({ default: m.TeamsPage })));

export const teamsPlugin: PluginDefinition = definePlugin({
  manifest: manifest as any,
  renderView: TeamsPage,
  apply: () => {
    return () => {};
  },
});

export default teamsPlugin;
