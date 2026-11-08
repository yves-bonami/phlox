(() => {
  "use strict";
  const svg = document.getElementById("flow-canvas");
  if (!svg) return;
  const connections = document.getElementById("connections");
  const palette = document.getElementById("node-palette");
  const status = document.getElementById("status");
  const count = document.getElementById("node-count");
  const nodes = new Map();
  const links = [];
  let dragging = null, wiring = null, selected = null, suppressClick = false;
  let sequence = 0;
  let selectedLink = null;
  const preview = document.createElementNS("http://www.w3.org/2000/svg", "path");
  preview.setAttribute("class", "connection connection-preview");
  preview.style.pointerEvents = "none";

  function element(tag, attributes = {}, text = null) {
    const item = document.createElementNS("http://www.w3.org/2000/svg", tag);
    Object.entries(attributes).forEach(([key, value]) => item.setAttribute(key, value));
    if (text !== null) item.textContent = text;
    return item;
  }
  function point(event) {
    const p = svg.createSVGPoint();
    p.x = event.clientX;
    p.y = event.clientY;
    return p.matrixTransform(svg.getScreenCTM().inverse());
  }
  function clearSelected() {
    if (selected) selected.classList.remove("selected");
    selected = null;
  }
  function addNode(type, x, y) {
    const id = "node-" + (++sequence);
    const group = element("g", { class: "node", "data-id": id });
    group.append(
      element("rect", { class: "node-body", width: 230, height: 120, rx: 12 }),
      element("rect", { class: "node-header", width: 230, height: 40, rx: 12 }),
      element("text", { class: "node-title", x: 18, y: 26 }, type.id.split("/").pop().slice(0, 22)),
      element("text", { class: "node-subtitle", x: 18, y: 79 }, type.id.slice(0, 30))
    );
    const ports = type.schema?.["x-phlox-ports"] || {};
    const inputs = Array.isArray(ports.inputs) ? ports.inputs : [];
    const outputs = Array.isArray(ports.outputs) ? ports.outputs : [];
    const renderPorts = (items, kind) => {
      items.forEach((port, index) => {
        if (typeof port.name !== "string") return;
        const py = 91 + index * 24;
        const px = kind === "input" ? 0 : 230;
        group.append(
          element("circle", { class: "port port-" + kind, "data-node": id, "data-port": port.name, cx: px, cy: py, r: 9 }),
          element("text", { class: "port-label", x: kind === "input" ? 20 : 210, y: py + 4,
            "text-anchor": kind === "input" ? "start" : "end" }, port.name)
        );
      });
    };
    renderPorts(inputs, "input");
    renderPorts(outputs, "output");
    svg.append(group);
    nodes.set(id, { group, x, y, type: type.id, inputs, outputs });
    updateNodes();
    status.textContent = "Added " + type.id + ".";
  }
  function path(startX, startY, endX, endY) {
    const curve = Math.max(65, Math.abs(endX - startX) * 0.5);
    return `M ${startX} ${startY} C ${startX + curve} ${startY}, ${endX - curve} ${endY}, ${endX} ${endY}`;
  }
  function updateNodes() {
    nodes.forEach(node => node.group.setAttribute("transform", `translate(${node.x} ${node.y})`));
    connections.replaceChildren();
    for (const link of links) {
      const from = nodes.get(link.from), to = nodes.get(link.to);
      if (!from || !to) continue;
      const x1 = from.x + 230, y1 = from.y + 91 + from.outputs.findIndex(p => p.name === link.fromPort) * 24;
      const x2 = to.x, y2 = to.y + 91 + to.inputs.findIndex(p => p.name === link.toPort) * 24;
      const d = path(x1, y1, x2, y2);
      const wire = element("g", { class: "wire" + (selectedLink === link ? " wire-selected" : "") });
      wire.append(
        element("path", { class: "connection-hit", d, "data-link": links.indexOf(link) }),
        element("path", { class: "connection", d, "pointer-events": "none" })
      );
      connections.append(wire);
      if (selectedLink === link) {
        const midX = (x1 + x2) / 2, midY = (y1 + y2) / 2;
        const remove = element("g", { class: "connection-remove", role: "button", tabindex: "0", "aria-label": "Remove selected connection" });
        remove.append(
          element("circle", { cx: midX, cy: midY, r: 15 }),
          element("text", { x: midX, y: midY + 5, "text-anchor": "middle" }, "×")
        );
        connections.append(remove);
      }
    }
    if (wiring?.moved) drawPreview(wiring.current);
    count.textContent = nodes.size + " nodes · Drag to arrange";
  }
  function drawPreview(at) {
    const source = nodes.get(wiring.from);
    if (!source) return;
    const portIndex = source.outputs.findIndex(p => p.name === wiring.fromPort);
    preview.setAttribute("d", path(source.x + 230, source.y + 91 + portIndex * 24, at.x, at.y));
    connections.append(preview);
  }
  function removeSelectedLink() {
    if (!selectedLink) return;
    const index = links.indexOf(selectedLink);
    if (index >= 0) links.splice(index, 1);
    selectedLink = null;
    status.textContent = "Connection removed.";
    updateNodes();
  }
  function connect(from, fromPort, to, toPort) {
    const sourceType = nodes.get(from)?.outputs.find(p => p.name === fromPort)?.value_type;
    const targetType = nodes.get(to)?.inputs.find(p => p.name === toPort)?.value_type;
    if (!nodes.has(from) || !nodes.has(to) || from === to ||
        (sourceType && targetType && sourceType !== targetType) ||
        links.some(l => l.from === from && l.fromPort === fromPort && l.to === to && l.toPort === toPort)) {
      status.textContent = "Connection already exists or is invalid.";
      return;
    }
    selectedLink = null;
    links.push({ from, fromPort, to, toPort });
    status.textContent = "Connected " + from + " → " + to;
    updateNodes();
  }
  function portAt(event) {
    return document.elementFromPoint(event.clientX, event.clientY)?.closest(".port-input");
  }
  svg.addEventListener("pointerdown", event => {
    if (event.button !== 0) return;
    if (event.target.closest(".connection-hit, .connection-remove")) return;
    const output = event.target.closest(".port-output");
    if (output) {
      event.preventDefault();
      wiring = { from: output.dataset.node, fromPort: output.dataset.port, pointerId: event.pointerId, start: point(event), moved: false };
      svg.setPointerCapture(event.pointerId);
      return;
    }
    if (event.target.closest(".port")) return;
    const group = event.target.closest(".node");
    if (!group) return;
    const p = point(event), node = nodes.get(group.dataset.id);
    dragging = { node, pointerId: event.pointerId, dx: p.x - node.x, dy: p.y - node.y };
    group.classList.add("dragging");
    svg.setPointerCapture(event.pointerId);
  });
  svg.addEventListener("pointermove", event => {
    if (wiring?.pointerId === event.pointerId) {
      const p = point(event);
      wiring.current = p;
      if (Math.hypot(p.x - wiring.start.x, p.y - wiring.start.y) > 5) wiring.moved = true;
      if (wiring.moved) drawPreview(p);
      return;
    }
    if (dragging?.pointerId !== event.pointerId) return;
    const p = point(event);
    dragging.node.x = Math.max(0, Math.min(730, p.x - dragging.dx));
    dragging.node.y = Math.max(0, Math.min(420, p.y - dragging.dy));
    updateNodes();
  });
  function endPointer(event, cancelled = false) {
    if (dragging?.pointerId === event.pointerId) {
      dragging.node.group.classList.remove("dragging");
      dragging = null;
    }
    if (wiring?.pointerId === event.pointerId) {
      if (wiring.moved) {
        const target = !cancelled ? portAt(event) : null;
        suppressClick = true;
        setTimeout(() => { suppressClick = false; }, 0);
        if (target) {
          connect(wiring.from, wiring.fromPort, target.dataset.node, target.dataset.port);
          clearSelected();
        } else if (!cancelled) status.textContent = "Release over an input port to connect.";
      }
      preview.remove();
      wiring = null;
    }
    if (svg.hasPointerCapture(event.pointerId)) svg.releasePointerCapture(event.pointerId);
  }
  svg.addEventListener("pointerup", event => endPointer(event));
  svg.addEventListener("pointercancel", event => endPointer(event, true));
  svg.addEventListener("click", event => {
    if (suppressClick) return;
    if (event.target.closest(".connection-remove")) {
      removeSelectedLink();
      return;
    }
    const hit = event.target.closest(".connection-hit");
    if (hit) {
      selectedLink = links[Number(hit.dataset.link)] || null;
      clearSelected();
      status.textContent = "Connection selected — click × or press Delete to remove.";
      updateNodes();
      return;
    }
    const port = event.target.closest(".port");
    if (!port) {
      if (selectedLink) {
        selectedLink = null;
        updateNodes();
      }
      clearSelected();
      return;
    }
    selectedLink = null;
    updateNodes();
    if (port.classList.contains("port-output")) {
      clearSelected();
      selected = port;
      selected.classList.add("selected");
      status.textContent = "Select an input port or drag between ports.";
    } else if (selected) {
      connect(selected.dataset.node, selected.dataset.port, port.dataset.node, port.dataset.port);
      clearSelected();
    }
  });
  document.addEventListener("keydown", event => {
    if (event.target.closest("input, textarea, select, [contenteditable]")) return;
    if (event.key === "Escape") {
      clearSelected();
      selectedLink = null;
      updateNodes();
    }
    if (selectedLink && (event.key === "Delete" || event.key === "Backspace")) {
      event.preventDefault();
      removeSelectedLink();
    }
  });
  connections.addEventListener("keydown", event => {
    if (event.target.closest(".connection-remove") && (event.key === "Enter" || event.key === " ")) {
      event.preventDefault();
      removeSelectedLink();
    }
  });
  document.getElementById("reset").addEventListener("click", () => {
    nodes.forEach(node => node.group.remove());
    nodes.clear();
    links.length = 0;
    selectedLink = null;
    clearSelected();
    wiring = null;
    preview.remove();
    status.textContent = "Canvas reset.";
    updateNodes();
  });
  async function loadPalette() {
    palette.textContent = "Loading plugins…";
    try {
      const response = await fetch("/api/plugins", { cache: "no-store" });
      if (!response.ok) throw new Error("HTTP " + response.status);
      const plugins = await response.json();
      if (!Array.isArray(plugins)) throw new Error("Unexpected response");
      palette.replaceChildren();
      for (const plugin of plugins) {
        if (!Array.isArray(plugin.nodes)) continue;
        for (const type of plugin.nodes) {
          if (typeof type.id !== "string") continue;
          const button = document.createElement("button");
          button.type = "button";
          button.className = "palette-node";
          button.textContent = type.id;
          button.title = plugin.id + " v" + plugin.version;
          button.addEventListener("click", () => {
            const n = nodes.size;
            addNode(type, 100 + (n % 3) * 260, 110 + (Math.floor(n / 3) % 3) * 145);
          });
          palette.append(button);
        }
      }
      if (!palette.children.length) palette.textContent = "No plugin nodes found. Install a plugin and refresh.";
    } catch (error) {
      palette.textContent = "Could not load plugins: " + error.message;
    }
  }
  document.getElementById("refresh-plugins").addEventListener("click", loadPalette);
  document.getElementById("plugin-install-form").addEventListener("submit", async event => {
    event.preventDefault();
    const form = event.currentTarget;
    const button = form.querySelector("button");
    button.disabled = true;
    status.textContent = "Installing plugin…";
    try {
      const response = await fetch("/api/plugins/install", {
        method: "POST",
        body: new FormData(form)
      });
      if (!response.ok) throw new Error((await response.text()).slice(0, 300));
      form.reset();
      status.textContent = "Plugin installed. Refreshing palette…";
      // The registry watcher activates new plugins asynchronously.
      await new Promise(resolve => setTimeout(resolve, 350));
      await loadPalette();
      status.textContent = "Plugin installation completed.";
    } catch (error) {
      status.textContent = "Install failed: " + error.message;
    } finally {
      button.disabled = false;
    }
  });
  updateNodes();
  loadPalette();
})();
