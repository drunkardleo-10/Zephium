/* @refresh reload */
import { render } from "solid-js/web";
import App from "./app/App";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "./styles/global.css";

render(() => <App />, document.getElementById("root")!);
