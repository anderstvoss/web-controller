import { copyFile } from "node:fs/promises";

await copyFile(
  new URL("src/index.html", import.meta.url),
  new URL("dist/index.html", import.meta.url),
);
await copyFile(
  new URL("src/style.css", import.meta.url),
  new URL("dist/style.css", import.meta.url),
);
