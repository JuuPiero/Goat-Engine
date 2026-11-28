/* tslint:disable */
/* eslint-disable */

export class EditorEngine {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * `[{ id, name, path }]` of an animated root and the objects under it.
     */
    anim_paths(root: string): string;
    /**
     * Shows a clip (its JSON) at a time on the root's objects; `anim_preview_end` puts them back.
     */
    anim_preview(root: string, clip_json: string, time: number): void;
    anim_preview_end(): void;
    /**
     * Apply All: returns `{ key, text }` (the prefab file to write).
     */
    apply_prefab(id: string): string;
    /**
     * The Game view's own canvas (Unity: Scene and Game views are separate and can show side by
     * side). Without it the one canvas shows whichever view `set_scene_view` picks.
     */
    attach_game_canvas(canvas: HTMLCanvasElement): void;
    backend_name(): string;
    /**
     * Runs an Inspector button of a component (e.g. NavMeshSurface ▸ Bake); returns its message.
     */
    component_action(id: string, component: string, action: string): string;
    /**
     * `{ components: { name: { menu, defaults?, fields?, requires? } }, behaviours: [...] }`.
     */
    component_info(): string;
    /**
     * JSON array of the registered component names.
     */
    component_names(): string;
    /**
     * JSON `[{ name, value }]` of an entity's components.
     */
    components(id: string): string;
    /**
     * Console expression in the running game: its id (0 = not playing); answers come from
     * `console_results`.
     */
    console_eval(code: string): number;
    /**
     * `[{ id, text, ok }]` answered since the last call.
     */
    console_results(): string;
    /**
     * The selection as scene JSON (clipboard), or nothing.
     */
    copy_selection(): string | undefined;
    /**
     * Starts the engine on a canvas (WebGPU, or WebGL2 when unavailable).
     */
    static create(canvas: HTMLCanvasElement): Promise<EditorEngine>;
    create_entity(name: string, parent?: string | null): string;
    delete_entity(id: string): void;
    /**
     * Duplicates the selection; returns the new ids (JSON array).
     */
    duplicate(): string;
    /**
     * Ctrl+click on an edit point deletes it; `dry` only tells whether it would.
     */
    edit_delete(x: number, y: number, w: number, h: number, dry: boolean): boolean;
    /**
     * One frame (`dt` seconds).
     */
    frame(dt: number): void;
    frame_selected(): void;
    /**
     * Device Simulator: a button of the simulated device ("A", "Start", "Up"...).
     */
    game_button(name: string, down: boolean): void;
    /**
     * The pointer over the Game view (`nx, ny` 0..1 over its canvas, negative = outside;
     * `buttons` = bit 0 left, 1 right, 2 middle): display pixels for the game (the touch screen in
     * the 3DS layout, like the desktop player).
     */
    game_pointer(nx: number, ny: number, buttons: number): void;
    /**
     * Mouse wheel over the Game view (lines; y > 0 = away from the user).
     */
    game_scroll(dx: number, dy: number): void;
    /**
     * Device Simulator: circle pad (`right` false) / C-stick, -1..1 with y up.
     */
    game_stick(right: boolean, x: number, y: number): void;
    /**
     * Starts a gizmo drag if (x, y) (0..1 of a w × h view) is on a handle.
     */
    gizmo_begin(x: number, y: number, w: number, h: number): boolean;
    /**
     * snap = Ctrl held (increment snapping).
     */
    gizmo_drag(dx: number, dy: number, w: number, h: number, snap: boolean): void;
    gizmo_end(): void;
    /**
     * Pointer over the Scene view without a button: highlights the handle under it.
     */
    gizmo_hover(x: number, y: number, w: number, h: number): void;
    /**
     * JSON array of `{ id, name, parent, kind }`.
     */
    hierarchy(): string;
    /**
     * Drops a model / prefab at a view point (0..1); returns the new entity.
     */
    instantiate(key: string, x: number, y: number): string;
    is_paused(): boolean;
    is_playing(): boolean;
    key(code: string, down: boolean): void;
    open_scene(key: string): void;
    orbit(dx: number, dy: number): void;
    pan(dx: number, dy: number): void;
    /**
     * Pastes clipboard JSON under `parent`; returns the new ids (JSON array).
     */
    paste(json: string, parent?: string | null): string;
    /**
     * Picks at (x, y) in 0..1 of the view; selects and returns the entity id.
     */
    pick(x: number, y: number): string | undefined;
    /**
     * Play mode; `dual_3ds` shows the game as the 3DS's two screens.
     */
    play(dual_3ds: boolean): void;
    /**
     * Play mode: `[{ handle, name, key, active }]` of the loaded scenes (Hierarchy headers).
     */
    play_scenes(): string;
    /**
     * `{ root, key, overrides: [...] }` of the prefab instance an entity belongs to, or "null".
     */
    prefab_info(id: string): string;
    /**
     * The profiler as JSON: `{ sections: [[name, ms]], draw_calls, triangles, history: [{ s, d, t }] }`.
     */
    profiler(): string;
    /**
     * Makes a project file available to the engine (the UI reads files through Tauri).
     */
    put_file(key: string, bytes: Uint8Array | number[]): void;
    /**
     * Call before changing the scene (Undo goes back to this state).
     */
    record_undo(label: string): void;
    redo(): string | undefined;
    redo_label(): string | undefined;
    remove_component(id: string, name: string): void;
    rename(id: string, name: string): void;
    /**
     * The Scene view's canvas was resized.
     */
    resize(width: number, height: number): void;
    /**
     * The Game view's canvas was resized.
     */
    resize_game(width: number, height: number): void;
    /**
     * Revert All: returns the new root id.
     */
    revert_prefab(id: string): string;
    save_scene(): string;
    scene(): string | undefined;
    /**
     * JSON shapes the UI draws over the Scene view (handles, selection outlines, icons), in
     * pixels of a `w × h` view; `px, py` = pointer (pixels, negative = none).
     */
    scene_overlay(w: number, h: number, px: number, py: number): string;
    /**
     * An Inspector button of a script (`@button`): Edit mode returns the props to save (JSON),
     * Play mode "" (the running object gets the call).
     */
    script_button(id: string, index: number, method: string): string;
    /**
     * `{ kind: "behaviour" | "scriptableObject", fields: {...} }` or `{ error }` of a script.
     */
    script_info(src: string): string;
    select(id?: string | null): void;
    /**
     * Selects several entities (JSON array of ids; the last is active).
     */
    select_many(ids_json: string): void;
    selected(): string | undefined;
    /**
     * JSON array of the selected ids (active last).
     */
    selection(): string;
    /**
     * Unity's 2D button (orthographic view down −Z, pan / zoom only).
     */
    set_2d(on: boolean): void;
    /**
     * Unity's SetActive (the object and its children).
     */
    set_active(id: string, on: boolean): void;
    set_component(id: string, name: string, json: string): void;
    /**
     * The Game view's 3DS two-screen layout (also in edit mode); the Scene view is always one view.
     */
    set_dual_3ds(on: boolean): void;
    /**
     * Device Simulator: room between the 3DS screens (logical pixels) — the device's hinge.
     */
    set_dual_gap(gap: number): void;
    /**
     * Edit Collider ("collider") / Edit Points ("points") on the selected object, "" = off.
     */
    set_edit_tool(tool: string): void;
    /**
     * The Console's Error Pause: pause when a script fails.
     */
    set_error_pause(on: boolean): void;
    /**
     * "move" / "rotate" / "scale" / "transform".
     */
    set_gizmo_mode(mode: string): void;
    set_gizmos(on: boolean): void;
    set_grid(on: boolean): void;
    /**
     * Persp / Iso.
     */
    set_ortho(on: boolean): void;
    set_parent(id: string, parent?: string | null): void;
    /**
     * Pauses / resumes Play mode (the Game view keeps drawing).
     */
    set_paused(paused: boolean): void;
    /**
     * The canvas shows the Scene view (true) or the Game view (false).
     */
    set_scene_view(on: boolean): void;
    /**
     * Device Simulator: the 3D slider (0..1): the game reads it (`Screen.slider3D`) and the top
     * screen previews the stereo pair as a red / cyan anaglyph (`anaglyph` false = slider only).
     */
    set_slider_3d(v: number, anaglyph: boolean): void;
    /**
     * Sets the tag ("Untagged" / empty removes it).
     */
    set_tag(id: string, tag: string): void;
    /**
     * Tool handle settings: Local / Global axes, Center / Pivot, snap increments.
     */
    set_tool_handles(local: boolean, center: boolean, snap_move: number, snap_rotate: number, snap_scale: number): void;
    set_transform(id: string, json: string): void;
    /**
     * Which views are on screen (hidden dock tabs aren't drawn).
     */
    set_views(scene: boolean, game: boolean): void;
    /**
     * `{ name, kind, properties, state, … }` or `{ errors }` of a `.shader` file.
     */
    shader_info(key: string): string;
    /**
     * Draw calls / triangles of the last frame.
     */
    stats(): string;
    /**
     * One frame while paused (Unity's Step).
     */
    step(): void;
    stop(): void;
    /**
     * Engine log lines since the last call: `[[level, text], ...]` (JSON).
     */
    take_logs(): string;
    /**
     * Why the game paused itself since the last call (Debug.break / Error Pause), "" = it didn't.
     */
    take_pause_reason(): string;
    /**
     * `{ position, euler, scale }` of an entity.
     */
    transform(id: string): string | undefined;
    undo(): string | undefined;
    undo_label(): string | undefined;
    /**
     * View gizmo click: "x" / "-x" / "y" / "-y" / "z" / "-z".
     */
    view_axis(axis: string): void;
    /**
     * { rotation: [x, y, z, w], distance, view: { mode2d, ortho, grid, gizmos, scene_view } }
     */
    view_state(): string;
    zoom(delta: number): void;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_editorengine_free: (a: number, b: number) => void;
    readonly editorengine_anim_paths: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_anim_preview: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
    readonly editorengine_anim_preview_end: (a: number) => void;
    readonly editorengine_apply_prefab: (a: number, b: number, c: number) => [number, number, number, number];
    readonly editorengine_attach_game_canvas: (a: number, b: any) => [number, number];
    readonly editorengine_backend_name: (a: number) => [number, number];
    readonly editorengine_component_action: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number, number, number];
    readonly editorengine_component_info: (a: number) => [number, number];
    readonly editorengine_component_names: (a: number) => [number, number];
    readonly editorengine_components: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_console_eval: (a: number, b: number, c: number) => number;
    readonly editorengine_console_results: (a: number) => [number, number];
    readonly editorengine_copy_selection: (a: number) => [number, number];
    readonly editorengine_create: (a: any) => any;
    readonly editorengine_create_entity: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly editorengine_delete_entity: (a: number, b: number, c: number) => void;
    readonly editorengine_duplicate: (a: number) => [number, number, number, number];
    readonly editorengine_edit_delete: (a: number, b: number, c: number, d: number, e: number, f: number) => number;
    readonly editorengine_frame: (a: number, b: number) => void;
    readonly editorengine_frame_selected: (a: number) => void;
    readonly editorengine_game_button: (a: number, b: number, c: number, d: number) => void;
    readonly editorengine_game_pointer: (a: number, b: number, c: number, d: number) => void;
    readonly editorengine_game_scroll: (a: number, b: number, c: number) => void;
    readonly editorengine_game_stick: (a: number, b: number, c: number, d: number) => void;
    readonly editorengine_gizmo_begin: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly editorengine_gizmo_drag: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly editorengine_gizmo_end: (a: number) => void;
    readonly editorengine_gizmo_hover: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_hierarchy: (a: number) => [number, number];
    readonly editorengine_instantiate: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly editorengine_is_paused: (a: number) => number;
    readonly editorengine_is_playing: (a: number) => number;
    readonly editorengine_key: (a: number, b: number, c: number, d: number) => void;
    readonly editorengine_open_scene: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_orbit: (a: number, b: number, c: number) => void;
    readonly editorengine_pan: (a: number, b: number, c: number) => void;
    readonly editorengine_paste: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly editorengine_pick: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_play: (a: number, b: number) => void;
    readonly editorengine_play_scenes: (a: number) => [number, number];
    readonly editorengine_prefab_info: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_profiler: (a: number) => [number, number];
    readonly editorengine_put_file: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_record_undo: (a: number, b: number, c: number) => void;
    readonly editorengine_redo: (a: number) => [number, number];
    readonly editorengine_redo_label: (a: number) => [number, number];
    readonly editorengine_remove_component: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_rename: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_resize: (a: number, b: number, c: number) => void;
    readonly editorengine_resize_game: (a: number, b: number, c: number) => void;
    readonly editorengine_revert_prefab: (a: number, b: number, c: number) => [number, number, number, number];
    readonly editorengine_save_scene: (a: number) => [number, number, number, number];
    readonly editorengine_scene: (a: number) => [number, number];
    readonly editorengine_scene_overlay: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly editorengine_script_button: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number, number];
    readonly editorengine_script_info: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_select: (a: number, b: number, c: number) => void;
    readonly editorengine_select_many: (a: number, b: number, c: number) => void;
    readonly editorengine_selected: (a: number) => [number, number];
    readonly editorengine_selection: (a: number) => [number, number];
    readonly editorengine_set_2d: (a: number, b: number) => void;
    readonly editorengine_set_active: (a: number, b: number, c: number, d: number) => void;
    readonly editorengine_set_component: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number];
    readonly editorengine_set_dual_3ds: (a: number, b: number) => void;
    readonly editorengine_set_dual_gap: (a: number, b: number) => void;
    readonly editorengine_set_edit_tool: (a: number, b: number, c: number) => void;
    readonly editorengine_set_error_pause: (a: number, b: number) => void;
    readonly editorengine_set_gizmo_mode: (a: number, b: number, c: number) => void;
    readonly editorengine_set_gizmos: (a: number, b: number) => void;
    readonly editorengine_set_grid: (a: number, b: number) => void;
    readonly editorengine_set_ortho: (a: number, b: number) => void;
    readonly editorengine_set_parent: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_set_paused: (a: number, b: number) => void;
    readonly editorengine_set_scene_view: (a: number, b: number) => void;
    readonly editorengine_set_slider_3d: (a: number, b: number, c: number) => void;
    readonly editorengine_set_tag: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_set_tool_handles: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly editorengine_set_transform: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly editorengine_set_views: (a: number, b: number, c: number) => void;
    readonly editorengine_shader_info: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_stats: (a: number) => [number, number];
    readonly editorengine_step: (a: number) => void;
    readonly editorengine_stop: (a: number) => void;
    readonly editorengine_take_logs: (a: number) => [number, number];
    readonly editorengine_take_pause_reason: (a: number) => [number, number];
    readonly editorengine_transform: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_undo: (a: number) => [number, number];
    readonly editorengine_undo_label: (a: number) => [number, number];
    readonly editorengine_view_axis: (a: number, b: number, c: number) => void;
    readonly editorengine_view_state: (a: number) => [number, number];
    readonly editorengine_zoom: (a: number, b: number) => void;
    readonly goat_action_triggered: (a: number, b: number) => number;
    readonly goat_action_value: (a: number, b: number, c: number) => void;
    readonly goat_button: (a: number, b: number) => number;
    readonly goat_call: (a: number, b: number, c: number, d: number, e: number, f: number) => number;
    readonly goat_delta_time: (a: number) => number;
    readonly goat_destroy: (a: number, b: number) => void;
    readonly goat_exists: (a: number, b: number) => number;
    readonly goat_find: (a: number, b: number) => number;
    readonly goat_get_component: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly goat_get_euler: (a: number, b: number, c: number) => void;
    readonly goat_get_name: (a: number, b: number, c: number, d: number) => number;
    readonly goat_get_parent: (a: number, b: number) => number;
    readonly goat_get_position: (a: number, b: number, c: number) => void;
    readonly goat_get_scale: (a: number, b: number, c: number) => void;
    readonly goat_get_world_position: (a: number, b: number, c: number) => void;
    readonly goat_instantiate: (a: number, b: number, c: number) => number;
    readonly goat_log: (a: number, b: number, c: number) => void;
    readonly goat_register_behaviour: (a: number) => void;
    readonly goat_remove_component: (a: number, b: number, c: number) => void;
    readonly goat_rotate: (a: number, b: number, c: number) => void;
    readonly goat_set_component: (a: number, b: number, c: number, d: number) => number;
    readonly goat_set_euler: (a: number, b: number, c: number) => void;
    readonly goat_set_name: (a: number, b: number, c: number) => void;
    readonly goat_set_parent: (a: number, b: number, c: number) => void;
    readonly goat_set_position: (a: number, b: number, c: number) => void;
    readonly goat_set_scale: (a: number, b: number, c: number) => void;
    readonly goat_set_time_scale: (a: number, b: number) => void;
    readonly goat_spawn: (a: number, b: number) => number;
    readonly goat_stick: (a: number, b: number) => void;
    readonly goat_time: (a: number) => number;
    readonly goat_time_scale: (a: number) => number;
    readonly goat_unscaled_delta_time: (a: number) => number;
    readonly wasm_bindgen_49c42322a91a8d99___convert__closures_____invoke___js_sys_1f8126b8a616f9cf___Function_fn_wasm_bindgen_49c42322a91a8d99___JsValue_____wasm_bindgen_49c42322a91a8d99___sys__Undefined___js_sys_1f8126b8a616f9cf___Function_fn_wasm_bindgen_49c42322a91a8d99___JsValue_____wasm_bindgen_49c42322a91a8d99___sys__Undefined_______true_: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen_49c42322a91a8d99___convert__closures_____invoke___wasm_bindgen_49c42322a91a8d99___JsValue__core_9b3796e30d99ddb7___result__Result_____wasm_bindgen_49c42322a91a8d99___JsError___true_: (a: number, b: number, c: any) => [number, number];
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
