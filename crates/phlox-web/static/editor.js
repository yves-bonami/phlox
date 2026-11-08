(() => {
  "use strict";
  const svg = document.getElementById("flow-canvas");
  if (!svg) return;
  const connections = document.getElementById("connections");
  const status = document.getElementById("status");
  const nodes = Array.from(svg.querySelectorAll(".node"));
  const positions = { source: { x: 140, y: 180 }, sink: { x: 570, y: 180 } };
  const links = [];
  let dragging = null;
  let selected = null;

  function point(event) {
    const p = svg.createSVGPoint();
    p.x = event.clientX;
    p.y = event.clientY;
    return p.matrixTransform(svg.getScreenCTM().inverse());
  }
  function updateNodes() {
    nodes.forEach(n => {
      const { x, y } = positions[n.dataset.id];
      n.setAttribute("transform", `translate(${x} ${y})`);
    });
    connections.replaceChildren();
    links.forEach(link => {
      const from = positions[link.from];
      const to = positions[link.to];
      const startX = from.x + 230, startY = from.y + 91;
      const endX = to.x, endY = to.y + 91;
      const curve = Math.max(65, Math.abs(endX - startX) * 0.5);
      const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
      path.setAttribute("class", "connection");
      path.setAttribute("d", `M ${startX} ${startY} C ${startX + curve} ${startY}, ${endX - curve} ${endY}, ${endX} ${endY}`);
      connections.append(path);
    });
  }
  let wiring = null;
  let suppressClick = false;
  const preview = document.createElementNS("http://www.w3.org/2000/svg", "path");
  preview.setAttribute("class", "connection connection-preview");
  preview.style.pointerEvents = "none";

  function clearSelected() {
    if (selected) selected.classList.remove("selected");
    selected = null;
  }
  function connect(from, to) {
    if (from === to || links.some(l => l.from === from && l.to === to)) {
      status.textContent = "Connection already exists or is invalid.";
      return;
    }
    links.push({ from, to });
    status.textContent = "Connected " + from + " → " + to;
    updateNodes();
  }
  function drawPreview(at) {
    const from = positions[wiring.from];
    const x = from.x + 230, y = from.y + 91;
    const curve = Math.max(65, Math.abs(at.x - x) * 0.5);
    preview.setAttribute("d", `M ${x} ${y} C ${x + curve} ${y}, ${at.x - curve} ${at.y}, ${at.x} ${at.y}`);
    connections.append(preview);
  }
  function portAt(event) {
    // Pointer capture retargets events to the SVG: hit-test using screen coordinates.
    const element = document.elementFromPoint(event.clientX, event.clientY);
    return element?.closest(".port-input");
  }
  svg.addEventListener("pointerdown", event => {
    if (event.button !== 0) return;
    const output = event.target.closest(".port-output");
    if (output) {
      event.preventDefault();
      wiring = { from: output.dataset.node, pointerId: event.pointerId, start: point(event), moved: false };
      svg.setPointerCapture(event.pointerId);
      return;
    }
    if (event.target.closest(".port")) return;
    const node = event.target.closest(".node");
    if (!node) return;
    const p = point(event);
    const pos = positions[node.dataset.id];
    dragging = { node, pointerId: event.pointerId, offsetX: p.x - pos.x, offsetY: p.y - pos.y };
    node.classList.add("dragging");
    svg.setPointerCapture(event.pointerId);
  });
  svg.addEventListener("pointermove", event => {
    if (wiring?.pointerId === event.pointerId) {
      const p = point(event);
      if (Math.hypot(p.x - wiring.start.x, p.y - wiring.start.y) > 5) wiring.moved = true;
      if (wiring.moved) drawPreview(p);
      return;
    }
    if (!dragging || dragging.pointerId !== event.pointerId) return;
    const p = point(event);
    positions[dragging.node.dataset.id] = {
      x: Math.max(0, Math.min(730, p.x - dragging.offsetX)),
      y: Math.max(0, Math.min(420, p.y - dragging.offsetY))
    };
    updateNodes();
  });
  function endPointer(event, cancelled = false) {
    if (dragging?.pointerId === event.pointerId) {
      dragging.node.classList.remove("dragging");
      dragging = null;
    }
    if (wiring?.pointerId === event.pointerId) {
      const target = !cancelled && wiring.moved ? portAt(event) : null;
      if (wiring.moved) {
        suppressClick = true;
        // The subsequent synthetic click belongs to this gesture, not port selection.
        setTimeout(() => { suppressClick = false; }, 0);
        if (target) {
          connect(wiring.from, target.dataset.node);
          clearSelected();
        } else if (!cancelled) {
          status.textContent = "Release over an input port to connect.";
        }
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
    const port = event.target.closest(".port");
    if (!port) return;
    if (port.classList.contains("port-output")) {
      clearSelected();
      selected = port;
      selected.classList.add("selected");
      status.textContent = "Select an input port to connect or drag between ports.";
    } else if (selected) {
      connect(selected.dataset.node, port.dataset.node);
      clearSelected();
    }
  });
  document.getElementById("reset").addEventListener("click", () => {
    for (const n of nodes) {
      positions[n.dataset.id] = { ...({ source: { x: 140, y: 180 }, sink: { x: 570, y: 180 } })[n.dataset.id] };
    }
    links.length = 0;
    clearSelected();
    preview.remove();
    wiring = null;
    status.textContent = "Canvas reset.";
    updateNodes();
  });
  updateNodes();
})();
