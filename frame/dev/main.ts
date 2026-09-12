import { mount } from "svelte";
import "@fontsource-variable/inter";
import "./preview.css";
import WorkPreview from "./WorkPreview.svelte";
const target = document.getElementById("app");
if (target) mount(WorkPreview, { target });
