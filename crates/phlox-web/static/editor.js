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
  let selectedNode = null;
  const deleteNodeButton = document.getElementById("delete-node");
  const propertiesPanel = document.getElementById("node-properties");
  const propertiesTitle = document.getElementById("node-properties-title");
  const propertiesFields = document.getElementById("node-properties-fields");
  function renderProperties() {
    propertiesPanel.hidden = !selectedNode;
    propertiesFields.replaceChildren();
    if (!selectedNode) return;
    const node = nodes.get(selectedNode);
    propertiesTitle.textContent = node.type;
    for (const [name, property] of Object.entries(node.schema?.properties || {})) {
      if (property.type !== "integer") continue;
      const label = document.createElement("label");
      label.textContent = name.replaceAll("_", " ");
      const input = document.createElement("input");
      input.type = "number";
      input.min = property.minimum ?? 1;
      input.max = property.maximum ?? 60000;
      input.value = node.configuration[name] ?? property.default ?? "";
      input.addEventListener("change", () => {
        const value = Number(input.value);
        if (Number.isInteger(value) && value >= Number(input.min) && value <= Number(input.max)) {
          node.configuration[name] = value;
          status.textContent = node.type + ": " + name + " = " + value;
        } else {
          input.value = node.configuration[name] ?? property.default ?? "";
        }
      });
      label.append(input);
      propertiesFields.append(label);
    }
  }
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
  function selectNode(id) {
    if (selectedNode) nodes.get(selectedNode)?.group.classList.remove("node-selected");
    selectedNode = nodes.has(id) ? id : null;
    if (selectedNode) nodes.get(selectedNode).group.classList.add("node-selected");
    deleteNodeButton.disabled = !selectedNode;
    renderProperties();
  }
  function deleteSelectedNode() {
    if (!selectedNode) return;
    const id = selectedNode;
    const node = nodes.get(id);
    node.group.remove();
    nodes.delete(id);
    for (let i = links.length - 1; i >= 0; i--) {
      if (links[i].from === id || links[i].to === id) links.splice(i, 1);
    }
    selectedLink = null;
    clearSelected();
    selectNode(null);
    status.textContent = "Deleted " + node.type + " and its connections.";
    updateNodes();
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
        const configuration = {};
    for (const [name, prop] of Object.entries(type.schema?.properties || {})) {
      if (prop.default !== undefined) configuration[name] = prop.default;
    }
    nodes.set(id, { group, x, y, type: type.id, inputs, outputs, schema: type.schema, configuration });
    updateNodes();
    selectNode(id);
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
    selectNode(group.dataset.id);
    selectedLink = null;
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
      const group = event.target.closest(".node");
      selectNode(group?.dataset.id || null);
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
      selectNode(null);
      clearSelected();
      selectedLink = null;
      updateNodes();
    }
    if (event.key === "Delete" || event.key === "Backspace") {
      if (selectedLink || selectedNode) event.preventDefault();
      if (selectedLink) removeSelectedLink();
      else deleteSelectedNode();
    }
  });
  connections.addEventListener("keydown", event => {
    if (event.target.closest(".connection-remove") && (event.key === "Enter" || event.key === " ")) {
      event.preventDefault();
      removeSelectedLink();
    }
  });
  deleteNodeButton.addEventListener("click", deleteSelectedNode);
  document.getElementById("reset").addEventListener("click", () => {
    nodes.forEach(node => node.group.remove());
    nodes.clear();
    selectNode(null);
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
      const knownTypes = new Map();
      for (const plugin of plugins) {
        if (!Array.isArray(plugin.nodes)) continue;
        for (const type of plugin.nodes) {
          if (typeof type.id !== "string") continue;
          knownTypes.set(type.id, type);
          const button = document.createElement("button");
          button.type = "button";
          button.draggable = true;
          button.className = "palette-node";
          button.addEventListener("dragstart", event => {
            event.dataTransfer.setData("application/x-phlox-node", type.id);
            event.dataTransfer.effectAllowed = "copy";
          });
          button.textContent = type.id;
          button.title = plugin.id + " v" + plugin.version;
          button.addEventListener("click", () => {
            const n = nodes.size;
            addNode(type, 100 + (n % 3) * 260, 110 + (Math.floor(n / 3) % 3) * 145);
          });
          palette.append(button);
        }
      }
      palette._knownTypes = knownTypes;
      if (!palette.children.length) palette.textContent = "No plugin nodes found. Install a plugin and refresh.";
    } catch (error) {
      palette.textContent = "Could not load plugins: " + error.message;
    }
  }
  svg.addEventListener("dragover", event => {
    if (event.dataTransfer.types.includes("application/x-phlox-node")) {
      event.preventDefault();
      event.dataTransfer.dropEffect = "copy";
    }
  });
  svg.addEventListener("drop", event => {
    event.preventDefault();
    const id = event.dataTransfer.getData("application/x-phlox-node");
    const type = palette._knownTypes?.get(id);
    if (!type) return;
    const p = point(event);
    addNode(type, Math.max(0, Math.min(730, p.x - 115)), Math.max(0, Math.min(420, p.y - 60)));
  });
  document.getElementById("refresh-plugins").addEventListener("click", loadPalette);
  const startButton = document.getElementById("start-flow");
  const stopButton = document.getElementById("stop-flow");
  const flowLogs = document.getElementById("flow-logs");
  startButton.addEventListener("click", async () => {
    const flow = {
      nodes: Array.from(nodes, ([id, node]) => ({ id, type: node.type, configuration: node.configuration })),
      edges: links.map(link => ({ from: link.from, fromPort: link.fromPort, to: link.to, toPort: link.toPort }))
    };
    try {
      const response = await fetch("/api/flow/start", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(flow) });
      if (!response.ok) throw new Error(await response.text());
      status.textContent = "Flow started.";
    } catch (error) { status.textContent = "Start failed: " + error.message; }
  });
  stopButton.addEventListener("click", async () => {
    await fetch("/api/flow/stop", { method: "POST" });
    status.textContent = "Flow stopped.";
  });
  async function refreshRun() {
    try {
      const response = await fetch("/api/flow/status");
      if (!response.ok) return;
      const state = await response.json();
      startButton.disabled = state.running;
      stopButton.disabled = !state.running;
      flowLogs.textContent = state.logs.join("\n");
    } catch { /* server is temporarily unavailable */ }
  }
  setInterval(refreshRun, 500);
  refreshRun();
  updateNodes();
  loadPalette();
})();
