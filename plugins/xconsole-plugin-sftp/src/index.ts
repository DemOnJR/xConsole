import { definePlugin, type PluginDefinition } from "../../../src/sdk/plugin";
import { SftpNode } from "./SftpNode";
import manifest from "../plugin.json";

export const sftpPlugin: PluginDefinition = definePlugin({
  manifest: manifest as any,
  renderNode: SftpNode,
  apply: () => {
    console.log(`[Plugin Harness] SFTP FTP plugin mounted`);
    return () => {
      console.log(`[Plugin Harness] SFTP FTP plugin unmounted`);
    };
  },
});

export default sftpPlugin;
