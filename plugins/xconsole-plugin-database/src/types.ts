export interface DbConnectionConfig {
  id: string;
  name: string;
  kind: "mysql" | "postgres" | "sqlite" | "redis";
  host?: string;
  port?: number;
  user?: string;
  password?: string;
  database?: string;
  filePath?: string;
}

export interface DbTableColumn {
  name: string;
  type: string;
  nullable: boolean;
  isPrimary: boolean;
  defaultValue?: string;
}

export interface DbTableInfo {
  name: string;
  columns: DbTableColumn[];
  rowCount?: number;
}

