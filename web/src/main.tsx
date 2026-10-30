import "./index.css";

import { render } from "@solidjs/web";
import { App } from "./app/App";

const root = document.getElementById("root");
if (!root) {
  throw new Error("index.html has no #root element");
}
render(() => <App />, root);
