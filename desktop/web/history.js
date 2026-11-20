// Undo/redo for the card creator.
//
// Rather than hooking every function that edits the card, this watches for
// user activity (typing, clicks, drags, drops) and, once things go quiet,
// records a snapshot of the global `card` object if it changed. Undo and
// redo restore a snapshot with loadCardData(), the same code that loads
// saved cards.
//
// Large strings (uploaded art is stored as multi-MB data: URLs) are stored
// once and shared between snapshots, so a long history stays small.
(function () {
	'use strict';

	var MAX_STEPS = 100;
	var QUIET_MS = 500;        // record this long after the last activity
	var FOLLOW_UP_MS = 2000;   // re-check later to fold in async results (e.g. auto-fit after an image loads)
	var SETTLE_MS = 400;       // let images and frame scripts finish after a restore
	var BIG_STRING = 4096;
	var TOKEN = '\u0001cc-shared:';

	var entries = [];   // serialized snapshots; entries[index] is the current state
	var index = -1;
	var restoring = false;
	var burst = 0;      // increments with each user action
	var lastRecordedBurst = -1;
	var quietTimer = null;
	var followUpTimer = null;

	// --- serialization with shared large strings ---------------------------

	var tokenFor = new Map();
	var stringFor = new Map();

	function share(value) {
		var token = tokenFor.get(value);
		if (!token) {
			token = TOKEN + tokenFor.size;
			tokenFor.set(value, token);
			stringFor.set(token, value);
		}
		return token;
	}

	function serialize() {
		if (typeof card !== 'object' || !card) {
			return null;
		}
		return JSON.stringify(card, function (key, value) {
			if (value instanceof HTMLImageElement || value instanceof HTMLCanvasElement) {
				return undefined; // rebuilt from the src when restoring, as with saved cards
			}
			if (typeof value === 'string' && value.length > BIG_STRING) {
				return share(value);
			}
			return value;
		});
	}

	function deserialize(snapshot) {
		return JSON.parse(snapshot, function (key, value) {
			if (typeof value === 'string' && value.startsWith(TOKEN)) {
				return stringFor.get(value);
			}
			return value;
		});
	}

	// --- recording -------------------------------------------------------

	function record() {
		clearTimeout(quietTimer);
		quietTimer = null;
		if (restoring) {
			return;
		}
		var snapshot = serialize();
		if (snapshot === null || snapshot === entries[index]) {
			return;
		}
		if (index === -1) {
			entries = [snapshot];
			index = 0;
		} else if (lastRecordedBurst === burst && index > 0 && index === entries.length - 1) {
			// Same user action as the last step (e.g. auto-fit finishing): update that step.
			entries[index] = snapshot;
		} else {
			entries.splice(index + 1);
			entries.push(snapshot);
			index++;
			if (entries.length > MAX_STEPS + 1) {
				entries.shift();
				index--;
			}
			lastRecordedBurst = burst;
		}
		updateButtons();
	}

	function captureBaseline() {
		if (index === -1 && !restoring) {
			var snapshot = serialize();
			if (snapshot !== null) {
				entries = [snapshot];
				index = 0;
				updateButtons();
			}
		}
	}

	function scheduleRecord() {
		clearTimeout(quietTimer);
		clearTimeout(followUpTimer);
		quietTimer = setTimeout(record, QUIET_MS);
		followUpTimer = setTimeout(record, FOLLOW_UP_MS);
	}

	function isOwnControl(event) {
		return event.target instanceof Element && event.target.closest('.cc-history-bar');
	}

	// Capture phase runs before the creator's own handlers, so the card
	// hasn't changed yet: the right moment to take a missing baseline.
	['pointerdown', 'keydown', 'input', 'drop'].forEach(function (type) {
		document.addEventListener(type, function (event) {
			if (!restoring && !isOwnControl(event)) {
				captureBaseline();
			}
		}, true);
	});
	['input', 'change', 'pointerup', 'keyup', 'drop', 'click', 'dblclick'].forEach(function (type) {
		document.addEventListener(type, function (event) {
			if (restoring || isOwnControl(event)) {
				return;
			}
			if (type === 'keyup' && (event.ctrlKey || event.metaKey)) {
				return;
			}
			burst++;
			scheduleRecord();
		}, true);
	});

	// --- undo / redo -----------------------------------------------------

	function flushPending() {
		// Record a not-yet-saved edit so undo goes back from it, not past it.
		if (quietTimer) {
			record();
		}
	}

	async function restore(newIndex) {
		restoring = true;
		clearTimeout(followUpTimer);
		index = newIndex;
		updateButtons();
		try {
			await loadCardData(deserialize(entries[index]), 'Undo step');
			await new Promise(function (resolve) { setTimeout(resolve, SETTLE_MS); });
			// Keep what the restore actually produced, so tiny rounding
			// differences aren't mistaken for a new edit later.
			var settled = serialize();
			if (settled !== null) {
				entries[index] = settled;
			}
		} catch (error) {
			notify('Undo/redo failed: ' + error, 6);
		} finally {
			restoring = false;
			lastRecordedBurst = -1;
			updateButtons();
		}
	}

	function undo() {
		if (restoring) {
			return;
		}
		flushPending();
		if (index > 0) {
			restore(index - 1);
		}
	}

	function redo() {
		if (restoring) {
			return;
		}
		flushPending();
		if (index < entries.length - 1) {
			restore(index + 1);
		}
	}

	function reset() {
		clearTimeout(quietTimer);
		clearTimeout(followUpTimer);
		quietTimer = null;
		entries = [];
		index = -1;
		lastRecordedBurst = -1;
		tokenFor.clear();
		stringFor.clear();
		updateButtons();
		setTimeout(captureBaseline, SETTLE_MS * 2);
	}

	// Ctrl+Z, Ctrl+Y, Ctrl+Shift+Z (Cmd on macOS). These undo card changes
	// even while typing in a text box.
	window.addEventListener('keydown', function (event) {
		if (!(event.ctrlKey || event.metaKey) || event.altKey) {
			return;
		}
		var key = event.key.toLowerCase();
		if (key === 'z' && !event.shiftKey) {
			event.preventDefault();
			event.stopPropagation();
			undo();
		} else if (key === 'y' || (key === 'z' && event.shiftKey)) {
			event.preventDefault();
			event.stopPropagation();
			redo();
		}
	}, true);

	// --- toolbar under the card preview ----------------------------------

	var undoButton, redoButton;

	var ICON_UNDO = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11"/></svg>';
	var ICON_REDO = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 14 5-5-5-5"/><path d="M20 9H9.5a5.5 5.5 0 0 0 0 11H13"/></svg>';

	function makeButton(label, icon, title, action, iconFirst) {
		var button = document.createElement('button');
		button.type = 'button';
		button.className = 'cc-history-button';
		button.title = title;
		button.setAttribute('aria-label', label);
		button.innerHTML = iconFirst ? icon + '<span>' + label + '</span>' : '<span>' + label + '</span>' + icon;
		button.addEventListener('click', action);
		return button;
	}

	function buildToolbar() {
		var canvas = document.querySelector('#previewCanvas');
		if (!canvas) {
			return;
		}
		// Wrap the canvas so the bar sits under it without disturbing the
		// creator's two-column grid.
		var column = document.createElement('div');
		column.className = 'cc-preview-column';
		canvas.parentNode.insertBefore(column, canvas);
		column.appendChild(canvas);

		var bar = document.createElement('div');
		bar.className = 'cc-history-bar';
		bar.setAttribute('role', 'toolbar');
		bar.setAttribute('aria-label', 'Undo and redo');
		undoButton = makeButton('Undo', ICON_UNDO, 'Undo (Ctrl+Z)', undo, true);
		redoButton = makeButton('Redo', ICON_REDO, 'Redo (Ctrl+Y)', redo, false);
		bar.appendChild(undoButton);
		bar.appendChild(redoButton);
		column.appendChild(bar);
		updateButtons();
	}

	function updateButtons() {
		if (!undoButton) {
			return;
		}
		undoButton.disabled = restoring || index <= 0;
		redoButton.disabled = restoring || index >= entries.length - 1;
	}

	buildToolbar();
	// Baseline once the starting frame pack and saved-card list have loaded.
	setTimeout(captureBaseline, 1500);

	window.cardHistory = { undo: undo, redo: redo, reset: reset };
})();
