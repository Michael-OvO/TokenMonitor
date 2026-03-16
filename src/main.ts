import App from "./App.svelte";
import { mount } from "svelte";
import "./app.css";

// Disable right-click context menu and browser reload shortcuts
document.addEventListener("contextmenu", (e) => e.preventDefault());
document.addEventListener("keydown", (e) => {
  if (
    (e.key === "r" && (e.metaKey || e.ctrlKey)) ||
    e.key === "F5"
  ) {
    e.preventDefault();
  }
});

const app = mount(App, { target: document.getElementById("app")! });

export default app;
