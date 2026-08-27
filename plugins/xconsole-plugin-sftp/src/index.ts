import { definePlugin, type PluginDefinition } from "../../../src/sdk/plugin";
import { SftpNode } from "../../../src/components/SftpNode";
import manifest from "../plugin.json";

export const sftpPlugin: PluginDefinition = definePlugin({
  manifest: manifest as any,
  renderCanvasNode: SftpNode,
  apply: () => {
    console.log(`[Plugin Harness] SFTP plugin mounted into Cordis context`);
    return () => {
      console.log(`[Plugin Harness] SFTP plugin unmounted from Cordis context`);
    };
  },
});

export default sftpPlugin;
