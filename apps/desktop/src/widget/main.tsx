import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../styles/app.css";
import { Widget } from "./Widget";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Widget />
  </StrictMode>,
);
