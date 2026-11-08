(() => {
  "use strict";
  const installed = document.getElementById("installed-plugins");
  const status = document.getElementById("status");
  async function refresh() {
    try {
      const response = await fetch("/api/plugins", { cache: "no-store" });
      if (!response.ok) throw new Error("HTTP " + response.status);
      const plugins = await response.json();
      installed.replaceChildren();
      for (const plugin of plugins) {
        const item = document.createElement("div");
        item.className = "installed-plugin";
        const heading = document.createElement("strong");
        heading.textContent = plugin.id + " v" + plugin.version;
        const details = document.createElement("p");
        details.textContent = (plugin.nodes || []).map(node => node.id).join(", ") || "No node types";
        item.append(heading, details);
        installed.append(item);
      }
      if (!plugins.length) installed.textContent = "No plugins installed.";
    } catch (error) {
      installed.textContent = "Unable to load plugins: " + error.message;
    }
  }
  document.getElementById("refresh-plugins").addEventListener("click", refresh);
  document.getElementById("plugin-install-form").addEventListener("submit", async event => {
    event.preventDefault();
    const form = event.currentTarget;
    const button = form.querySelector("button");
    button.disabled = true;
    status.textContent = "Installing…";
    try {
      const response = await fetch("/api/plugins/install", { method: "POST", body: new FormData(form) });
      if (!response.ok) throw new Error((await response.text()).slice(0, 300));
      form.reset();
      status.textContent = "Installed. Refresh to see the plugin after discovery.";
      await refresh();
    } catch (error) {
      status.textContent = "Installation failed: " + error.message;
    } finally {
      button.disabled = false;
    }
  });
  refresh();
})();
