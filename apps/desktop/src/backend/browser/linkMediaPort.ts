import type { LinkMediaPort } from "../linkMediaPort";

export const browserLinkMediaPort: LinkMediaPort = {
  async openHttpUrl(url) {
    window.open(url, "_blank", "noopener,noreferrer");
  },
  mediaSourceUrl(sourceUrl) {
    return sourceUrl;
  },
  async saveMediaFile() {}
};
