export interface WindowDialogFilter {
  name: string;
  extensions: string[];
}

export interface WindowConfirmOptions {
  title: string;
  kind: "warning";
}

export interface WindowSaveFileOptions {
  title: string;
  defaultPath: string;
  filters: WindowDialogFilter[];
}

export interface WindowOpenFileOptions {
  title: string;
  filters: WindowDialogFilter[];
  multiple: boolean;
  fileAccessMode: "scoped";
}

export interface WindowDialogPort {
  toggleFullscreen(): Promise<void>;
  startDragging(): Promise<void>;
  confirm(message: string, options: WindowConfirmOptions): Promise<boolean>;
  saveFile(options: WindowSaveFileOptions): Promise<string | null>;
  openFile(options: WindowOpenFileOptions): Promise<string | string[] | null>;
}
