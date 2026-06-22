import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// NOTE: StrictMode is intentionally omitted. Its double-mount in development
// would open two SSH sessions per terminal node.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<App />);
