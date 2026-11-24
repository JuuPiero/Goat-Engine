/* tslint:disable */
/* eslint-disable */

export class EditorEngine {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    backend_name(): string;
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
     * One frame (`dt` seconds).
     */
    frame(dt: number): void;
    frame_selected(): void;
    /**
     * Starts a gizmo drag if (x, y) (0..1 of a w × h view) is on a handle.
     */
    gizmo_begin(x: number, y: number, w: number, h: number): boolean;
    gizmo_drag(dx: number, dy: number, w: number, h: number): void;
    gizmo_end(): void;
    /**
     * JSON array of `{ id, name, parent, kind }`.
     */
    hierarchy(): string;
    /**
     * Drops a model / prefab at a view point (0..1); returns the new entity.
     */
    instantiate(key: string, x: number, y: number): string;
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
    resize(width: number, height: number): void;
    save_scene(): string;
    scene(): string | undefined;
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
    set_component(id: string, name: string, json: string): void;
    /**
     * "move" / "rotate" / "scale".
     */
    set_gizmo_mode(mode: string): void;
    set_parent(id: string, parent?: string | null): void;
    set_transform(id: string, json: string): void;
    /**
     * Draw calls / triangles of the last frame.
     */
    stats(): string;
    stop(): void;
    /**
     * Engine log lines since the last call: `[[level, text], ...]` (JSON).
     */
    take_logs(): string;
    /**
     * `{ position, euler, scale }` of an entity.
     */
    transform(id: string): string | undefined;
    undo(): string | undefined;
    undo_label(): string | undefined;
    zoom(delta: number): void;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_editorengine_free: (a: number, b: number) => void;
    readonly editorengine_backend_name: (a: number) => [number, number];
    readonly editorengine_component_info: (a: number) => [number, number];
    readonly editorengine_component_names: (a: number) => [number, number];
    readonly editorengine_components: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_copy_selection: (a: number) => [number, number];
    readonly editorengine_create: (a: any) => any;
    readonly editorengine_create_entity: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly editorengine_delete_entity: (a: number, b: number, c: number) => void;
    readonly editorengine_duplicate: (a: number) => [number, number, number, number];
    readonly editorengine_frame: (a: number, b: number) => void;
    readonly editorengine_frame_selected: (a: number) => void;
    readonly editorengine_gizmo_begin: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly editorengine_gizmo_drag: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_gizmo_end: (a: number) => void;
    readonly editorengine_hierarchy: (a: number) => [number, number];
    readonly editorengine_instantiate: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly editorengine_is_playing: (a: number) => number;
    readonly editorengine_key: (a: number, b: number, c: number, d: number) => void;
    readonly editorengine_open_scene: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_orbit: (a: number, b: number, c: number) => void;
    readonly editorengine_pan: (a: number, b: number, c: number) => void;
    readonly editorengine_paste: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly editorengine_pick: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_play: (a: number, b: number) => void;
    readonly editorengine_put_file: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_record_undo: (a: number, b: number, c: number) => void;
    readonly editorengine_redo: (a: number) => [number, number];
    readonly editorengine_redo_label: (a: number) => [number, number];
    readonly editorengine_remove_component: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_rename: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_resize: (a: number, b: number, c: number) => void;
    readonly editorengine_save_scene: (a: number) => [number, number, number, number];
    readonly editorengine_scene: (a: number) => [number, number];
    readonly editorengine_select: (a: number, b: number, c: number) => void;
    readonly editorengine_select_many: (a: number, b: number, c: number) => void;
    readonly editorengine_selected: (a: number) => [number, number];
    readonly editorengine_selection: (a: number) => [number, number];
    readonly editorengine_set_component: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number];
    readonly editorengine_set_gizmo_mode: (a: number, b: number, c: number) => void;
    readonly editorengine_set_parent: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly editorengine_set_transform: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly editorengine_stats: (a: number) => [number, number];
    readonly editorengine_stop: (a: number) => void;
    readonly editorengine_take_logs: (a: number) => [number, number];
    readonly editorengine_transform: (a: number, b: number, c: number) => [number, number];
    readonly editorengine_undo: (a: number) => [number, number];
    readonly editorengine_undo_label: (a: number) => [number, number];
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
