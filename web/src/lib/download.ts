import type { Attachment } from "./api";

/**
 * Hands the browser a file to save, for downloads a plain link can't fetch (a POST, such as the
 * site's export with its secrets).
 */
export function saveFile(file: Attachment) {
  const url = URL.createObjectURL(file.blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = file.name;
  link.rel = "noopener";
  document.body.append(link);
  link.click();
  link.remove();
  // The click has handed the blob over; the URL may go once this task ends.
  setTimeout(() => URL.revokeObjectURL(url), 0);
}
