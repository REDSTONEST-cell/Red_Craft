import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import MobileApp from "./mobile/MobileApp.svelte";
import { fitViewport } from "./mobile/viewport";

// 手机版（Android）使用单独的界面
const mobile = /Android/i.test(navigator.userAgent);
if (mobile) fitViewport();

const app = mount(mobile ? MobileApp : App, { target: document.getElementById("app")! });

export default app;
