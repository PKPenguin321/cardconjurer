// Saved-card storage for the creator.
//
// In the desktop app, each saved card is a file on disk managed by the Rust
// side (src-tauri/src/cards.rs), so there is no 5 MB localStorage limit.
// In a normal browser it falls back to localStorage, same as the website.
//
// The list of card names is kept in memory so the creator can read it
// synchronously; card contents are only read from disk when needed.
(function () {
	'use strict';

	var tauri = window.__TAURI__ && window.__TAURI__.core;
	var keys = [];

	function sortKeys() {
		keys.sort();
	}
	function addKey(key) {
		if (keys.indexOf(key) === -1) {
			keys.push(key);
			sortKeys();
			return true;
		}
		return false;
	}
	function removeKey(key) {
		var index = keys.indexOf(key);
		if (index !== -1) {
			keys.splice(index, 1);
		}
	}
	function persistBrowserKeys() {
		if (!tauri) {
			localStorage.setItem('cardKeys', JSON.stringify(keys));
		}
	}

	var cardStore = {
		desktop: Boolean(tauri),

		// Resolves once the list of saved cards has been loaded.
		ready: (tauri
			? tauri.invoke('cards_list')
			: Promise.resolve(JSON.parse(localStorage.getItem('cardKeys') || '[]'))
		).then(function (loaded) {
			keys = loaded || [];
			sortKeys();
		}),

		// Names of all saved cards, sorted.
		keys: function () {
			return keys.slice();
		},

		// The saved card's JSON text, or null.
		get: async function (key) {
			return tauri ? tauri.invoke('cards_get', { key: key }) : localStorage.getItem(key);
		},

		// Saves a card's JSON text. The name is added to keys() immediately,
		// so several saves started together see each other.
		set: async function (key, data) {
			var isNew = addKey(key);
			try {
				if (tauri) {
					await tauri.invoke('cards_set', { key: key, data: data });
				} else {
					localStorage.setItem(key, data);
					persistBrowserKeys();
				}
			} catch (error) {
				if (isNew) {
					removeKey(key);
				}
				throw error;
			}
		},

		remove: async function (key) {
			removeKey(key);
			if (tauri) {
				await tauri.invoke('cards_remove', { key: key });
			} else {
				localStorage.removeItem(key);
				persistBrowserKeys();
			}
		},

		clear: async function () {
			var removed = keys;
			keys = [];
			if (tauri) {
				await tauri.invoke('cards_clear');
			} else {
				removed.forEach(function (key) { localStorage.removeItem(key); });
				persistBrowserKeys();
			}
		},

		// Folder holding saved cards (desktop only).
		folder: async function () {
			return tauri ? tauri.invoke('cards_folder') : null;
		}
	};

	window.cardStore = cardStore;
})();
