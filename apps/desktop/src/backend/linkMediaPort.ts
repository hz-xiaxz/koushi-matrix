export interface LinkMediaPort {
  openHttpUrl(url: string): Promise<void>;
  mediaSourceUrl(sourceUrl: string): string;
  saveMediaFile(sourceUrl: string, filename: string): Promise<void>;
}
