export interface SftpFileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified: number;
  permissions?: string;
}

export interface SftpTransferItem {
  id: string;
  sourcePath: string;
  destPath: string;
  direction: "upload" | "download";
  status: "pending" | "transferring" | "completed" | "error";
  progress: number;
  error?: string;
}
