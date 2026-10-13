import { RawInterpreter } from './snippets/dioxus-interpreter-js-20451f6aefec841d/inline0.js';
import { setAttributeInner } from './snippets/dioxus-interpreter-js-20451f6aefec841d/src/js/set_attribute.js';
import { get_select_data } from './snippets/dioxus-web-eae67878b22d6805/inline0.js';


/**
 * Mount the page into the element `root`, with the lessons, `lessons`
 * (lessons.json), and `machine`, the kernel's files arriving (a promise of
 * its image, tables, initramfs and disk: the page boots it; the lesson
 * shows meanwhile).
 * @param {string} root
 * @param {string} lessons
 * @param {Promise<any>} machine
 */
export function mount(root, lessons, machine) {
    const ptr0 = passStringToWasm0(root, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(lessons, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ret = wasm.mount(ptr0, len0, ptr1, len1, machine);
    if (ret[1]) {
        throw takeFromExternrefTable0(ret[0]);
    }
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_debug_string_0e68cf47c9cbd9b0: function(arg0, arg1) {
            const ret = debugString(arg1);
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_exports_3d3410a3b95d7b41: function() {
            const ret = wasm;
            return ret;
        },
        __wbg___wbindgen_is_function_fcda5e3902d732fe: function(arg0) {
            const ret = typeof(arg0) === 'function';
            return ret;
        },
        __wbg___wbindgen_is_undefined_8c687d0b90d5b524: function(arg0) {
            const ret = arg0 === undefined;
            return ret;
        },
        __wbg___wbindgen_memory_3f8442e22540244f: function() {
            const ret = wasm.memory;
            return ret;
        },
        __wbg___wbindgen_number_get_1dc732b810cb937c: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'number' ? obj : undefined;
            getDataViewMemory0().setFloat64(arg0 + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_string_get_92ab86bb19cbc12f: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_5d9e815e6fdf150f: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg__wbg_cb_unref_997e73d32238e655: function(arg0) {
            arg0._wbg_cb_unref();
        },
        __wbg_activeElement_a40cd04e4e5cfaff: function(arg0) {
            const ret = arg0.activeElement;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_addEventListener_9534e2dc12044ee4: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            arg0.addEventListener(getStringFromWasm0(arg1, arg2), arg3);
        }, arguments); },
        __wbg_altKey_8b8e9b0d63b945bb: function(arg0) {
            const ret = arg0.altKey;
            return ret;
        },
        __wbg_altKey_c75bf4903a5db4c2: function(arg0) {
            const ret = arg0.altKey;
            return ret;
        },
        __wbg_altKey_fa381deee4b395cd: function(arg0) {
            const ret = arg0.altKey;
            return ret;
        },
        __wbg_animationName_5f6e148b26109948: function(arg0, arg1) {
            const ret = arg1.animationName;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_appendChild_0a61ee9a9da4556f: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.appendChild(arg1);
            return ret;
        }, arguments); },
        __wbg_arrayBuffer_432de2b072afe7fb: function(arg0) {
            const ret = arg0.arrayBuffer();
            return ret;
        },
        __wbg_beginPath_ccad41b15817641f: function(arg0) {
            arg0.beginPath();
        },
        __wbg_blockSize_726f0afadf8c1f3f: function(arg0) {
            const ret = arg0.blockSize;
            return ret;
        },
        __wbg_blur_c712b137baa8b295: function() { return handleError(function (arg0) {
            arg0.blur();
        }, arguments); },
        __wbg_borderBoxSize_62234ce3ef78f33f: function(arg0) {
            const ret = arg0.borderBoxSize;
            return ret;
        },
        __wbg_bottom_89fa307d0439be2c: function(arg0) {
            const ret = arg0.bottom;
            return ret;
        },
        __wbg_boundingClientRect_ea4c4d9e9e5be8cb: function(arg0) {
            const ret = arg0.boundingClientRect;
            return ret;
        },
        __wbg_bubbles_454fa86c9a7c87f4: function(arg0) {
            const ret = arg0.bubbles;
            return ret;
        },
        __wbg_button_f6aa74c65b739064: function(arg0) {
            const ret = arg0.button;
            return ret;
        },
        __wbg_buttons_fc06c2cbd043b4af: function(arg0) {
            const ret = arg0.buttons;
            return ret;
        },
        __wbg_call_269c5566fbede3eb: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.call(arg1);
            return ret;
        }, arguments); },
        __wbg_call_6bcf8d3e20937e46: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.call(arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_call_7bbd9cceba9949ad: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = arg0.call(arg1, arg2, arg3);
            return ret;
        }, arguments); },
        __wbg_call_c1ad1cb1b78e8130: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            const ret = arg0.call(arg1, arg2, arg3, arg4);
            return ret;
        }, arguments); },
        __wbg_changedTouches_b982f6f6739b3f06: function(arg0) {
            const ret = arg0.changedTouches;
            return ret;
        },
        __wbg_checkValidity_530e82b8322fc0b3: function(arg0) {
            const ret = arg0.checkValidity();
            return ret;
        },
        __wbg_checked_ab679020f304032a: function(arg0) {
            const ret = arg0.checked;
            return ret;
        },
        __wbg_className_ff521206be3457a9: function(arg0, arg1) {
            const ret = arg1.className;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_clearData_20da58828b911102: function() { return handleError(function (arg0) {
            arg0.clearData();
        }, arguments); },
        __wbg_clearData_34c16f21ab9a9d3b: function() { return handleError(function (arg0, arg1, arg2) {
            arg0.clearData(getStringFromWasm0(arg1, arg2));
        }, arguments); },
        __wbg_clearRect_60e914c5e35403c7: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.clearRect(arg1, arg2, arg3, arg4);
        },
        __wbg_click_842edec52299234d: function(arg0) {
            arg0.click();
        },
        __wbg_clientHeight_8669994caeda015c: function(arg0) {
            const ret = arg0.clientHeight;
            return ret;
        },
        __wbg_clientWidth_669677d957e958c7: function(arg0) {
            const ret = arg0.clientWidth;
            return ret;
        },
        __wbg_clientX_3cd572a5cd368348: function(arg0) {
            const ret = arg0.clientX;
            return ret;
        },
        __wbg_clientX_997500aa23d483d1: function(arg0) {
            const ret = arg0.clientX;
            return ret;
        },
        __wbg_clientY_4651eed688551d1c: function(arg0) {
            const ret = arg0.clientY;
            return ret;
        },
        __wbg_clientY_52b1b3958eb93e2d: function(arg0) {
            const ret = arg0.clientY;
            return ret;
        },
        __wbg_clip_b4135b1f98d3018c: function(arg0) {
            arg0.clip();
        },
        __wbg_closest_c70e5aadd2078e50: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.closest(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_code_de6c620a1a241f59: function(arg0, arg1) {
            const ret = arg1.code;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_compile_fe2895fcc4d85949: function(arg0) {
            const ret = WebAssembly.compile(arg0);
            return ret;
        },
        __wbg_construct_69525de4bcbd615c: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.construct(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_contentBoxSize_da8ed0423f37fa73: function(arg0) {
            const ret = arg0.contentBoxSize;
            return ret;
        },
        __wbg_createComment_2f844d723b100a86: function(arg0, arg1, arg2) {
            const ret = arg0.createComment(getStringFromWasm0(arg1, arg2));
            return ret;
        },
        __wbg_createElementNS_a45335f2f24f9ec5: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            const ret = arg0.createElementNS(arg1 === 0 ? undefined : getStringFromWasm0(arg1, arg2), getStringFromWasm0(arg3, arg4));
            return ret;
        }, arguments); },
        __wbg_createElement_b9024dc5ba95ac27: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.createElement(getStringFromWasm0(arg1, arg2));
            return ret;
        }, arguments); },
        __wbg_createTextNode_6d2db3073c3d6df6: function(arg0, arg1, arg2) {
            const ret = arg0.createTextNode(getStringFromWasm0(arg1, arg2));
            return ret;
        },
        __wbg_ctrlKey_785e802b8798af13: function(arg0) {
            const ret = arg0.ctrlKey;
            return ret;
        },
        __wbg_ctrlKey_ba07ac4b2ea90600: function(arg0) {
            const ret = arg0.ctrlKey;
            return ret;
        },
        __wbg_ctrlKey_df3eaa38d7d12de7: function(arg0) {
            const ret = arg0.ctrlKey;
            return ret;
        },
        __wbg_dataTransfer_9d0a7bfb637e95b6: function(arg0) {
            const ret = arg0.dataTransfer;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_data_3634610ec0d56ca6: function(arg0, arg1) {
            const ret = arg1.data;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_decodeURIComponent_1655d04feda9dfee: function() { return handleError(function (arg0, arg1) {
            const ret = decodeURIComponent(getStringFromWasm0(arg0, arg1));
            return ret;
        }, arguments); },
        __wbg_defaultPrevented_6532d5b02b675eeb: function(arg0) {
            const ret = arg0.defaultPrevented;
            return ret;
        },
        __wbg_deltaMode_1f7b0a593ff1ea16: function(arg0) {
            const ret = arg0.deltaMode;
            return ret;
        },
        __wbg_deltaX_948c3a4bd666561a: function(arg0) {
            const ret = arg0.deltaX;
            return ret;
        },
        __wbg_deltaY_635e57899c7ee3a1: function(arg0) {
            const ret = arg0.deltaY;
            return ret;
        },
        __wbg_deltaZ_9798555025e4da12: function(arg0) {
            const ret = arg0.deltaZ;
            return ret;
        },
        __wbg_detail_84fdc765141875ba: function(arg0) {
            const ret = arg0.detail;
            return ret;
        },
        __wbg_devicePixelRatio_da557bfb3b99fb02: function(arg0) {
            const ret = arg0.devicePixelRatio;
            return ret;
        },
        __wbg_dispatchEvent_67aab71ae1deb26a: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.dispatchEvent(arg1);
            return ret;
        }, arguments); },
        __wbg_documentElement_02547989338a4499: function(arg0) {
            const ret = arg0.documentElement;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_document_c7f486c52d63d24e: function(arg0) {
            const ret = arg0.document;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_done_cffed884d87aa22e: function(arg0) {
            const ret = arg0.done;
            return ret;
        },
        __wbg_dropEffect_3275a468cd6ac036: function(arg0, arg1) {
            const ret = arg1.dropEffect;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_effectAllowed_abf0839c8e0d3235: function(arg0, arg1) {
            const ret = arg1.effectAllowed;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_elapsedTime_e39b78687444eeeb: function(arg0) {
            const ret = arg0.elapsedTime;
            return ret;
        },
        __wbg_elapsedTime_f5498294beaabde3: function(arg0) {
            const ret = arg0.elapsedTime;
            return ret;
        },
        __wbg_elementFromPoint_519b61865fc6f04f: function(arg0, arg1, arg2) {
            const ret = arg0.elementFromPoint(arg1, arg2);
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_encodeURIComponent_d4884d77d1dfa553: function(arg0, arg1) {
            const ret = encodeURIComponent(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg_entries_4936217d46734891: function(arg0) {
            const ret = arg0.entries();
            return ret;
        },
        __wbg_error_756c5934221e6fee: function(arg0) {
            console.error(arg0);
        },
        __wbg_error_b7e73e671a58a488: function(arg0, arg1) {
            console.error(arg0, arg1);
        },
        __wbg_exports_89c3f1bc156ad5da: function(arg0) {
            const ret = arg0.exports;
            return ret;
        },
        __wbg_fetch_079086473aeec73d: function(arg0, arg1, arg2) {
            const ret = arg0.fetch(getStringFromWasm0(arg1, arg2));
            return ret;
        },
        __wbg_files_054ebec12ea09826: function(arg0) {
            const ret = arg0.files;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_files_06af76c704b59946: function(arg0) {
            const ret = arg0.files;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_fillRect_ff9957352a08db2c: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.fillRect(arg1, arg2, arg3, arg4);
        },
        __wbg_fillText_9b463cd65b9c1016: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.fillText(getStringFromWasm0(arg1, arg2), arg3, arg4);
        }, arguments); },
        __wbg_firstElementChild_c4359d5571e95230: function(arg0) {
            const ret = arg0.firstElementChild;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_focus_c1b2d4962209010b: function() { return handleError(function (arg0) {
            arg0.focus();
        }, arguments); },
        __wbg_force_a8103dca73d83d62: function(arg0) {
            const ret = arg0.force;
            return ret;
        },
        __wbg_getAsFile_e64a2869e682dcc6: function() { return handleError(function (arg0) {
            const ret = arg0.getAsFile();
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_getAttribute_b12f8535a2cb5368: function(arg0, arg1, arg2, arg3) {
            const ret = arg1.getAttribute(getStringFromWasm0(arg2, arg3));
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_getBoundingClientRect_299f40f8641457e6: function(arg0) {
            const ret = arg0.getBoundingClientRect();
            return ret;
        },
        __wbg_getComputedStyle_89f9e5c4de4b7edf: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.getComputedStyle(arg1);
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_getContext_e0c05ffee530bdcf: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.getContext(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_getData_b19c87e3887bc0cb: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = arg1.getData(getStringFromWasm0(arg2, arg3));
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        }, arguments); },
        __wbg_getElementById_ccc92d66acf76819: function(arg0, arg1, arg2) {
            const ret = arg0.getElementById(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_getItem_5d579ac5f70ace34: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = arg1.getItem(getStringFromWasm0(arg2, arg3));
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        }, arguments); },
        __wbg_getNode_b14ec18ffa4892a9: function(arg0, arg1) {
            const ret = arg0.getNode(arg1 >>> 0);
            return ret;
        },
        __wbg_getPropertyValue_778f881c5d299730: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = arg1.getPropertyValue(getStringFromWasm0(arg2, arg3));
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        }, arguments); },
        __wbg_get_501ad1d6b5ba20fe: function(arg0, arg1) {
            const ret = arg0[arg1 >>> 0];
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_get_989d0a1309644f2b: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_get_b1f0ab13c737f856: function(arg0, arg1) {
            const ret = arg0[arg1 >>> 0];
            return ret;
        },
        __wbg_get_c3db89fbf291e5ab: function(arg0, arg1) {
            const ret = arg0[arg1 >>> 0];
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_get_select_data_44a585f337de4182: function(arg0, arg1) {
            const ret = get_select_data(arg1);
            const ptr1 = passArrayJsValueToWasm0(ret, wasm.__wbindgen_malloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_get_unchecked_363572bdd397d473: function(arg0, arg1) {
            const ret = arg0[arg1 >>> 0];
            return ret;
        },
        __wbg_grow_fc949adcbe8a9b1d: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.grow(arg1 >>> 0);
            return ret;
        }, arguments); },
        __wbg_hash_04b938b4a921e95e: function() { return handleError(function (arg0, arg1) {
            const ret = arg1.hash;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        }, arguments); },
        __wbg_height_24586a23e4789d28: function(arg0) {
            const ret = arg0.height;
            return ret;
        },
        __wbg_height_d011730144e088ea: function(arg0) {
            const ret = arg0.height;
            return ret;
        },
        __wbg_height_f390cf417f3cf99d: function(arg0) {
            const ret = arg0.height;
            return ret;
        },
        __wbg_history_9d7bf9676b7c736c: function() { return handleError(function (arg0) {
            const ret = arg0.history;
            return ret;
        }, arguments); },
        __wbg_identifier_33e3b1ed04ae35c8: function(arg0) {
            const ret = arg0.identifier;
            return ret;
        },
        __wbg_imports_e1230ba6af31ae91: function(arg0) {
            const ret = WebAssembly.Module.imports(arg0);
            return ret;
        },
        __wbg_initialize_6d1129af221f598b: function(arg0, arg1, arg2) {
            arg0.initialize(arg1, arg2);
        },
        __wbg_inlineSize_199278605b83587a: function(arg0) {
            const ret = arg0.inlineSize;
            return ret;
        },
        __wbg_innerHeight_f0945dfd6f2d3a87: function() { return handleError(function (arg0) {
            const ret = arg0.innerHeight;
            return ret;
        }, arguments); },
        __wbg_innerWidth_1553ed0783afe5b4: function() { return handleError(function (arg0) {
            const ret = arg0.innerWidth;
            return ret;
        }, arguments); },
        __wbg_instanceof_AnimationEvent_fd2fa1d231fd0d23: function(arg0) {
            let result;
            try {
                result = arg0 instanceof AnimationEvent;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Blob_38df052cf775f822: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Blob;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_CanvasRenderingContext2d_bde5245b14027f3f: function(arg0) {
            let result;
            try {
                result = arg0 instanceof CanvasRenderingContext2D;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_CompositionEvent_179872ea787b2ebe: function(arg0) {
            let result;
            try {
                result = arg0 instanceof CompositionEvent;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_CustomEvent_f65ee8ab12ac87f9: function(arg0) {
            let result;
            try {
                result = arg0 instanceof CustomEvent;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Document_9284a3da8a2e4796: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Document;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_DragEvent_0aaee068b6b4ba41: function(arg0) {
            let result;
            try {
                result = arg0 instanceof DragEvent;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Element_c95c45635d03047e: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Element;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Event_a4e4dbe20b01838b: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Event;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_File_388508da6a606d0d: function(arg0) {
            let result;
            try {
                result = arg0 instanceof File;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_FocusEvent_1322b267a999bc0d: function(arg0) {
            let result;
            try {
                result = arg0 instanceof FocusEvent;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_HtmlCanvasElement_4d7e131643d814c1: function(arg0) {
            let result;
            try {
                result = arg0 instanceof HTMLCanvasElement;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_HtmlElement_bd89ce6abf9dad44: function(arg0) {
            let result;
            try {
                result = arg0 instanceof HTMLElement;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_HtmlFormElement_de556433be5fcd15: function(arg0) {
            let result;
            try {
                result = arg0 instanceof HTMLFormElement;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_HtmlInputElement_086894dc9c868a64: function(arg0) {
            let result;
            try {
                result = arg0 instanceof HTMLInputElement;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_HtmlSelectElement_2be52e2158c4f399: function(arg0) {
            let result;
            try {
                result = arg0 instanceof HTMLSelectElement;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_HtmlTextAreaElement_632129eba25e4367: function(arg0) {
            let result;
            try {
                result = arg0 instanceof HTMLTextAreaElement;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_KeyboardEvent_e7ef7527c166dc71: function(arg0) {
            let result;
            try {
                result = arg0 instanceof KeyboardEvent;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_MouseEvent_7b6ebd01ef5907f7: function(arg0) {
            let result;
            try {
                result = arg0 instanceof MouseEvent;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Node_5b8bea1a7005fbb9: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Node;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_PointerEvent_4269e92f16379aea: function(arg0) {
            let result;
            try {
                result = arg0 instanceof PointerEvent;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Response_6366a785e400039b: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Response;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_TouchEvent_19242dc3c8d02052: function(arg0) {
            let result;
            try {
                result = arg0 instanceof TouchEvent;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_TransitionEvent_12d35df2a2af4c7c: function(arg0) {
            let result;
            try {
                result = arg0 instanceof TransitionEvent;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Uint8Array_598adc0fef426aa8: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Uint8Array;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_WheelEvent_82ec56ef53ac3877: function(arg0) {
            let result;
            try {
                result = arg0 instanceof WheelEvent;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Window_a3b8566f0a9c5d1a: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Window;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instantiate_884602f4730130a7: function(arg0, arg1) {
            const ret = WebAssembly.instantiate(arg0, arg1);
            return ret;
        },
        __wbg_intersectionRatio_dcd7062de83ff1ca: function(arg0) {
            const ret = arg0.intersectionRatio;
            return ret;
        },
        __wbg_intersectionRect_574cd4412c445618: function(arg0) {
            const ret = arg0.intersectionRect;
            return ret;
        },
        __wbg_isArray_5674713bb7b79043: function(arg0) {
            const ret = Array.isArray(arg0);
            return ret;
        },
        __wbg_isComposing_2ab72d608a0a8d0e: function(arg0) {
            const ret = arg0.isComposing;
            return ret;
        },
        __wbg_isIntersecting_4b154fb6df9d9a6d: function(arg0) {
            const ret = arg0.isIntersecting;
            return ret;
        },
        __wbg_isPrimary_6a50a5efd7086e89: function(arg0) {
            const ret = arg0.isPrimary;
            return ret;
        },
        __wbg_item_5ad6a68f51ea4b33: function(arg0, arg1) {
            const ret = arg0.item(arg1 >>> 0);
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_item_adf6c9a98cc42542: function(arg0, arg1) {
            const ret = arg0.item(arg1 >>> 0);
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_items_a8fdc4cf728ef922: function(arg0) {
            const ret = arg0.items;
            return ret;
        },
        __wbg_key_d29d670923b81510: function(arg0, arg1) {
            const ret = arg1.key;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_kind_ed2088ba17535d91: function(arg0, arg1) {
            const ret = arg1.kind;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_lastModified_a90bb7a34b384061: function(arg0) {
            const ret = arg0.lastModified;
            return ret;
        },
        __wbg_left_644de747afd08e57: function(arg0) {
            const ret = arg0.left;
            return ret;
        },
        __wbg_length_2a004c1681a133a1: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_length_31bdaf014f5fbde2: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_length_4e1adc0d42e23620: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_length_b9633f9f50fac3c7: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_length_c0ad739f2efc455b: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_length_f97c9822ab91d4f4: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_localStorage_72d815b9c2adaaad: function() { return handleError(function (arg0) {
            const ret = arg0.localStorage;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_location_382075cf6a393ad4: function(arg0) {
            const ret = arg0.location;
            return ret;
        },
        __wbg_location_4cfd4b7727d1ad11: function(arg0) {
            const ret = arg0.location;
            return ret;
        },
        __wbg_log_363d83b9114c8831: function(arg0) {
            console.log(arg0);
        },
        __wbg_log_687b67a15dd8c94a: function(arg0, arg1) {
            console.log(arg0, arg1);
        },
        __wbg_message_1cbc5bc03dcf1dee: function(arg0) {
            const ret = arg0.message;
            return ret;
        },
        __wbg_metaKey_3f9183dfd5e1b726: function(arg0) {
            const ret = arg0.metaKey;
            return ret;
        },
        __wbg_metaKey_c085a7ea7d9587dd: function(arg0) {
            const ret = arg0.metaKey;
            return ret;
        },
        __wbg_metaKey_feb71e77f26b1a4c: function(arg0) {
            const ret = arg0.metaKey;
            return ret;
        },
        __wbg_name_2290eb6a0f9bb82e: function(arg0, arg1) {
            const ret = arg1.name;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_name_9240af9c79ef35aa: function(arg0, arg1) {
            const ret = arg1.name;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_new_1da3429bc3c4541c: function(arg0) {
            const ret = new Uint8Array(arg0);
            return ret;
        },
        __wbg_new_2c70a53214dd57eb: function() { return handleError(function (arg0, arg1) {
            const ret = new WebAssembly.Instance(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_new_8f72ed7c652bf5a2: function(arg0, arg1) {
            try {
                var state0 = {a: arg0, b: arg1};
                var cb0 = (arg0, arg1) => {
                    const a = state0.a;
                    state0.a = 0;
                    try {
                        return wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___js_sys_c16d4f21c4e547e2___Function_fn_wasm_bindgen_20ccca07c9e9b7d4___JsValue_____wasm_bindgen_20ccca07c9e9b7d4___sys__Undefined___js_sys_c16d4f21c4e547e2___Function_fn_wasm_bindgen_20ccca07c9e9b7d4___JsValue_____wasm_bindgen_20ccca07c9e9b7d4___sys__Undefined_______true_(a, state0.b, arg0, arg1);
                    } finally {
                        state0.a = a;
                    }
                };
                const ret = new Promise(cb0);
                return ret;
            } finally {
                state0.a = 0;
            }
        },
        __wbg_new_9952c6712c73a634: function() { return handleError(function () {
            const ret = new DataTransfer();
            return ret;
        }, arguments); },
        __wbg_new_bebc3f4757acf305: function() {
            const ret = new Object();
            return ret;
        },
        __wbg_new_c5dc7c0518fdb0ba: function(arg0) {
            const ret = new RawInterpreter(arg0 >>> 0);
            return ret;
        },
        __wbg_new_e9fffa3c467d7786: function() { return handleError(function () {
            const ret = new FileReader();
            return ret;
        }, arguments); },
        __wbg_new_eba495809764b79b: function() { return handleError(function (arg0, arg1) {
            const ret = new WebAssembly.Global(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_new_f3295e46c72eacf7: function() { return handleError(function (arg0) {
            const ret = new WebAssembly.Module(arg0);
            return ret;
        }, arguments); },
        __wbg_new_from_slice_4ee02165f9de919e: function(arg0, arg1) {
            const ret = new Uint8Array(getArrayU8FromWasm0(arg0, arg1));
            return ret;
        },
        __wbg_new_with_event_init_dict_054c74cc59208493: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = new Event(getStringFromWasm0(arg0, arg1), arg2);
            return ret;
        }, arguments); },
        __wbg_new_with_event_init_dict_1386a5d4e2b068c4: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = new PointerEvent(getStringFromWasm0(arg0, arg1), arg2);
            return ret;
        }, arguments); },
        __wbg_new_with_event_init_dict_37eaee3b66320633: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = new WheelEvent(getStringFromWasm0(arg0, arg1), arg2);
            return ret;
        }, arguments); },
        __wbg_new_with_form_dbba564808b01fa6: function() { return handleError(function (arg0) {
            const ret = new FormData(arg0);
            return ret;
        }, arguments); },
        __wbg_new_with_keyboard_event_init_dict_74ffd0372dee8fe6: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = new KeyboardEvent(getStringFromWasm0(arg0, arg1), arg2);
            return ret;
        }, arguments); },
        __wbg_new_with_mouse_event_init_dict_c478693bd150e5b7: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = new MouseEvent(getStringFromWasm0(arg0, arg1), arg2);
            return ret;
        }, arguments); },
        __wbg_next_f31ecb8646d2c605: function() { return handleError(function (arg0) {
            const ret = arg0.next();
            return ret;
        }, arguments); },
        __wbg_now_0b7736bc17fd7719: function(arg0) {
            const ret = arg0.now();
            return ret;
        },
        __wbg_of_1a3f47170341cefe: function(arg0) {
            const ret = Array.of(arg0);
            return ret;
        },
        __wbg_offsetX_5b637acbf4fc40e8: function(arg0) {
            const ret = arg0.offsetX;
            return ret;
        },
        __wbg_offsetY_ad9ca8949500ce96: function(arg0) {
            const ret = arg0.offsetY;
            return ret;
        },
        __wbg_ok_0d8e8fee3573b7ce: function(arg0) {
            const ret = arg0.ok;
            return ret;
        },
        __wbg_ownerDocument_5e91fd7a8aefc7cc: function(arg0) {
            const ret = arg0.ownerDocument;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_pageX_6b1a2c1b3f3be830: function(arg0) {
            const ret = arg0.pageX;
            return ret;
        },
        __wbg_pageX_7794a7bd22e64abf: function(arg0) {
            const ret = arg0.pageX;
            return ret;
        },
        __wbg_pageY_353ef292c5f0d0cd: function(arg0) {
            const ret = arg0.pageY;
            return ret;
        },
        __wbg_pageY_b268c6e19acefd4e: function(arg0) {
            const ret = arg0.pageY;
            return ret;
        },
        __wbg_parentElement_c0b090e8023d3df2: function(arg0) {
            const ret = arg0.parentElement;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_pathname_1c74e1a4da38e765: function() { return handleError(function (arg0, arg1) {
            const ret = arg1.pathname;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        }, arguments); },
        __wbg_performance_d96ad0b441f4510a: function(arg0) {
            const ret = arg0.performance;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_pointerId_b8c496f0172b1473: function(arg0) {
            const ret = arg0.pointerId;
            return ret;
        },
        __wbg_pointerType_753a9c4f0e3018cc: function(arg0, arg1) {
            const ret = arg1.pointerType;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_pressure_1021f185ca9ffe6b: function(arg0) {
            const ret = arg0.pressure;
            return ret;
        },
        __wbg_preventDefault_4235a4ccf8540533: function(arg0) {
            arg0.preventDefault();
        },
        __wbg_propertyName_cf392fa6f240f867: function(arg0, arg1) {
            const ret = arg1.propertyName;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_prototypesetcall_ae9f5e7459250748: function(arg0, arg1, arg2) {
            Uint8Array.prototype.set.call(getArrayU8FromWasm0(arg0, arg1), arg2);
        },
        __wbg_pseudoElement_262f4289f77f4f4b: function(arg0, arg1) {
            const ret = arg1.pseudoElement;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_pseudoElement_2ae0d1302ef6948a: function(arg0, arg1) {
            const ret = arg1.pseudoElement;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_querySelectorAll_fdbf93e11103921d: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.querySelectorAll(getStringFromWasm0(arg1, arg2));
            return ret;
        }, arguments); },
        __wbg_querySelector_5b1d9ebad373dc9d: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.querySelector(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_querySelector_e621a716a5ee2aa9: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.querySelector(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_queueMicrotask_85c90f6987555d65: function(arg0) {
            const ret = arg0.queueMicrotask;
            return ret;
        },
        __wbg_queueMicrotask_f6a1fa10b81d1fc0: function(arg0) {
            queueMicrotask(arg0);
        },
        __wbg_radiusX_4f13056638241bcc: function(arg0) {
            const ret = arg0.radiusX;
            return ret;
        },
        __wbg_radiusY_43aba04cd6f66d1d: function(arg0) {
            const ret = arg0.radiusY;
            return ret;
        },
        __wbg_readAsArrayBuffer_ef9c26393f5c58b1: function() { return handleError(function (arg0, arg1) {
            arg0.readAsArrayBuffer(arg1);
        }, arguments); },
        __wbg_readAsText_6aee528cd111ea76: function() { return handleError(function (arg0, arg1) {
            arg0.readAsText(arg1);
        }, arguments); },
        __wbg_rect_72ec16500f72e6f3: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.rect(arg1, arg2, arg3, arg4);
        },
        __wbg_releasePointerCapture_8978a2cfa472ae9a: function() { return handleError(function (arg0, arg1) {
            arg0.releasePointerCapture(arg1);
        }, arguments); },
        __wbg_removeAttribute_8ce0ab2fa42b383d: function() { return handleError(function (arg0, arg1, arg2) {
            arg0.removeAttribute(getStringFromWasm0(arg1, arg2));
        }, arguments); },
        __wbg_removeItem_350dd4387dc52350: function() { return handleError(function (arg0, arg1, arg2) {
            arg0.removeItem(getStringFromWasm0(arg1, arg2));
        }, arguments); },
        __wbg_remove_2d9ed324f8c6126a: function(arg0) {
            arg0.remove();
        },
        __wbg_repeat_01e85ccb1bcef326: function(arg0) {
            const ret = arg0.repeat;
            return ret;
        },
        __wbg_replaceState_6542bba3c2e92e5e: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.replaceState(arg1, getStringFromWasm0(arg2, arg3), arg4 === 0 ? undefined : getStringFromWasm0(arg4, arg5));
        }, arguments); },
        __wbg_requestAnimationFrame_4988887658fcee2e: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.requestAnimationFrame(arg1);
            return ret;
        }, arguments); },
        __wbg_resolve_35ec7e0c6af4c82c: function(arg0) {
            const ret = Promise.resolve(arg0);
            return ret;
        },
        __wbg_restore_eecc6aaa55774880: function(arg0) {
            arg0.restore();
        },
        __wbg_result_6a6fab012d5f9cc1: function() { return handleError(function (arg0) {
            const ret = arg0.result;
            return ret;
        }, arguments); },
        __wbg_right_3b45e4b285b3f490: function(arg0) {
            const ret = arg0.right;
            return ret;
        },
        __wbg_rootBounds_73318b4dba5bec4a: function(arg0) {
            const ret = arg0.rootBounds;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_rotationAngle_c94da25554ec8611: function(arg0) {
            const ret = arg0.rotationAngle;
            return ret;
        },
        __wbg_run_005df3744a5aa7cf: function(arg0) {
            arg0.run();
        },
        __wbg_saveTemplate_e3c1a1e2ac405343: function(arg0, arg1, arg2, arg3) {
            var v0 = getArrayJsValueFromWasm0(arg1, arg2);
            wasm.__wbindgen_free(arg1, arg2 * 4, 4);
            arg0.saveTemplate(v0, arg3);
        },
        __wbg_save_c33ea55c57a4e522: function(arg0) {
            arg0.save();
        },
        __wbg_screenX_174b66ca193d04ef: function(arg0) {
            const ret = arg0.screenX;
            return ret;
        },
        __wbg_screenX_ae667756b820f7c4: function(arg0) {
            const ret = arg0.screenX;
            return ret;
        },
        __wbg_screenY_5af62c383e06b59b: function(arg0) {
            const ret = arg0.screenY;
            return ret;
        },
        __wbg_screenY_dd328b61a5d3fe3f: function(arg0) {
            const ret = arg0.screenY;
            return ret;
        },
        __wbg_scrollHeight_8bdbf1cf581a2105: function(arg0) {
            const ret = arg0.scrollHeight;
            return ret;
        },
        __wbg_scrollHeight_b1a7772dca8769f0: function(arg0) {
            const ret = arg0.scrollHeight;
            return ret;
        },
        __wbg_scrollIntoView_326d8d17701e2108: function(arg0, arg1) {
            arg0.scrollIntoView(arg1);
        },
        __wbg_scrollIntoView_cd4b3768d08abd9f: function(arg0) {
            arg0.scrollIntoView();
        },
        __wbg_scrollLeft_5bebc8ba042c2371: function(arg0) {
            const ret = arg0.scrollLeft;
            return ret;
        },
        __wbg_scrollTop_77a6c2418331d800: function(arg0) {
            const ret = arg0.scrollTop;
            return ret;
        },
        __wbg_scrollTop_daa6298435db442a: function(arg0) {
            const ret = arg0.scrollTop;
            return ret;
        },
        __wbg_scrollWidth_a9140c22eb2b68c3: function(arg0) {
            const ret = arg0.scrollWidth;
            return ret;
        },
        __wbg_scroll_ced8afbb54bc6212: function(arg0, arg1) {
            arg0.scroll(arg1);
        },
        __wbg_selectionStart_41fde58c23be3c21: function() { return handleError(function (arg0) {
            const ret = arg0.selectionStart;
            return isLikeNone(ret) ? Number.MAX_SAFE_INTEGER : (ret) >>> 0;
        }, arguments); },
        __wbg_setAttributeInner_a29bef1cf5d53ffe: function(arg0, arg1, arg2, arg3, arg4, arg5) {
            setAttributeInner(arg0, getStringFromWasm0(arg1, arg2), arg3, arg4 === 0 ? undefined : getStringFromWasm0(arg4, arg5));
        },
        __wbg_setAttribute_8c84e9351986b1f6: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.setAttribute(getStringFromWasm0(arg1, arg2), getStringFromWasm0(arg3, arg4));
        }, arguments); },
        __wbg_setData_f099ea2a3a271149: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.setData(getStringFromWasm0(arg1, arg2), getStringFromWasm0(arg3, arg4));
        }, arguments); },
        __wbg_setItem_51f02f76b24c5d18: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.setItem(getStringFromWasm0(arg1, arg2), getStringFromWasm0(arg3, arg4));
        }, arguments); },
        __wbg_setPointerCapture_b8446eaac3edc11e: function() { return handleError(function (arg0, arg1) {
            arg0.setPointerCapture(arg1);
        }, arguments); },
        __wbg_setProperty_6139dd07828b5817: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.setProperty(getStringFromWasm0(arg1, arg2), getStringFromWasm0(arg3, arg4));
        }, arguments); },
        __wbg_setSelectionRange_f95d233357782d19: function() { return handleError(function (arg0, arg1, arg2) {
            arg0.setSelectionRange(arg1 >>> 0, arg2 >>> 0);
        }, arguments); },
        __wbg_setTransform_a79839e493a9863e: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6) {
            arg0.setTransform(arg1, arg2, arg3, arg4, arg5, arg6);
        }, arguments); },
        __wbg_set_361ed22a3b8802e9: function() { return handleError(function (arg0, arg1, arg2) {
            arg0.set(arg1 >>> 0, arg2);
        }, arguments); },
        __wbg_set_a377297433dfea63: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = Reflect.set(arg0, arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_set_behavior_7ea9ff4d9986a2a1: function(arg0, arg1) {
            arg0.behavior = __wbindgen_enum_ScrollBehavior[arg1];
        },
        __wbg_set_behavior_fc7a62e8f5217b27: function(arg0, arg1) {
            arg0.behavior = __wbindgen_enum_ScrollBehavior[arg1];
        },
        __wbg_set_block_6882e6bcd1f9b921: function(arg0, arg1) {
            arg0.block = __wbindgen_enum_ScrollLogicalPosition[arg1];
        },
        __wbg_set_bubbles_557a07ef6b7eaa99: function(arg0, arg1) {
            arg0.bubbles = arg1 !== 0;
        },
        __wbg_set_bubbles_aa24c8050a5232b7: function(arg0, arg1) {
            arg0.bubbles = arg1 !== 0;
        },
        __wbg_set_bubbles_bce4e096ca8d96aa: function(arg0, arg1) {
            arg0.bubbles = arg1 !== 0;
        },
        __wbg_set_bubbles_defdf15ece70296a: function(arg0, arg1) {
            arg0.bubbles = arg1 !== 0;
        },
        __wbg_set_bubbles_eb832f224b57131f: function(arg0, arg1) {
            arg0.bubbles = arg1 !== 0;
        },
        __wbg_set_cancelable_a70224aa34b4c0ed: function(arg0, arg1) {
            arg0.cancelable = arg1 !== 0;
        },
        __wbg_set_client_x_408a2e1222b5c519: function(arg0, arg1) {
            arg0.clientX = arg1;
        },
        __wbg_set_client_x_6dbefc7146cd529d: function(arg0, arg1) {
            arg0.clientX = arg1;
        },
        __wbg_set_client_x_ee5248b5338ce740: function(arg0, arg1) {
            arg0.clientX = arg1;
        },
        __wbg_set_client_y_a5413922910e5377: function(arg0, arg1) {
            arg0.clientY = arg1;
        },
        __wbg_set_client_y_b8c65239355d70d0: function(arg0, arg1) {
            arg0.clientY = arg1;
        },
        __wbg_set_client_y_c07b92b1a2319674: function(arg0, arg1) {
            arg0.clientY = arg1;
        },
        __wbg_set_delta_y_6fe7c8775656a5bb: function(arg0, arg1) {
            arg0.deltaY = arg1;
        },
        __wbg_set_dropEffect_9a560c6fdae25981: function(arg0, arg1, arg2) {
            arg0.dropEffect = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_effectAllowed_8add52099a6a9b26: function(arg0, arg1, arg2) {
            arg0.effectAllowed = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_fillStyle_59940a18480ccdf7: function(arg0, arg1, arg2) {
            arg0.fillStyle = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_font_50854c5bdd1ca607: function(arg0, arg1, arg2) {
            arg0.font = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_globalAlpha_fead6b3865673822: function(arg0, arg1) {
            arg0.globalAlpha = arg1;
        },
        __wbg_set_height_698fb3b255bc1348: function(arg0, arg1) {
            arg0.height = arg1 >>> 0;
        },
        __wbg_set_inline_4f25c8826ef77a01: function(arg0, arg1) {
            arg0.inline = __wbindgen_enum_ScrollLogicalPosition[arg1];
        },
        __wbg_set_key_3092a834af847934: function(arg0, arg1, arg2) {
            arg0.key = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_left_63bbb8fe4ecd10d4: function(arg0, arg1) {
            arg0.left = arg1;
        },
        __wbg_set_lineWidth_0b0f32c89c240172: function(arg0, arg1) {
            arg0.lineWidth = arg1;
        },
        __wbg_set_onload_b3d8ff8ccbd7017c: function(arg0, arg1) {
            arg0.onload = arg1;
        },
        __wbg_set_scrollTop_52f1d15e516eaa28: function(arg0, arg1) {
            arg0.scrollTop = arg1;
        },
        __wbg_set_scrollTop_b43e9ed358ec0b82: function(arg0, arg1) {
            arg0.scrollTop = arg1;
        },
        __wbg_set_shift_key_9a06dfee41d5ee2c: function(arg0, arg1) {
            arg0.shiftKey = arg1 !== 0;
        },
        __wbg_set_strokeStyle_2ce46a01ec1ff9ad: function(arg0, arg1, arg2) {
            arg0.strokeStyle = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_textBaseline_41538118f83d0399: function(arg0, arg1, arg2) {
            arg0.textBaseline = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_title_576991c4a4b3c882: function(arg0, arg1, arg2) {
            arg0.title = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_top_330d6e8d1d2f9427: function(arg0, arg1) {
            arg0.top = arg1;
        },
        __wbg_set_value_23f51d0287ae7cb7: function(arg0, arg1, arg2) {
            arg0.value = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_value_6eb2c83ae62c950f: function(arg0, arg1, arg2) {
            arg0.value = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_value_c3a663c8f495b99b: function(arg0, arg1) {
            arg0.value = arg1;
        },
        __wbg_set_width_4c3a2252e0dea033: function(arg0, arg1) {
            arg0.width = arg1 >>> 0;
        },
        __wbg_shiftKey_1daef5d8d06d4247: function(arg0) {
            const ret = arg0.shiftKey;
            return ret;
        },
        __wbg_shiftKey_2b651c3e5a9c07f5: function(arg0) {
            const ret = arg0.shiftKey;
            return ret;
        },
        __wbg_shiftKey_9dd20e0fb9da6eb5: function(arg0) {
            const ret = arg0.shiftKey;
            return ret;
        },
        __wbg_size_338f1717fbf84c4e: function(arg0) {
            const ret = arg0.size;
            return ret;
        },
        __wbg_static_accessor_GLOBAL_8eb4cd83130a11a0: function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_GLOBAL_THIS_1e7044f654e934db: function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_SELF_d8b50611246a6d92: function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_WINDOW_fd0bc376bf0f8b42: function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_strokeRect_a5ecc238952f1eb8: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.strokeRect(arg1, arg2, arg3, arg4);
        },
        __wbg_style_b79c69cee0c004b9: function(arg0) {
            const ret = arg0.style;
            return ret;
        },
        __wbg_subarray_1daff70dde20c145: function(arg0, arg1, arg2) {
            const ret = arg0.subarray(arg1 >>> 0, arg2 >>> 0);
            return ret;
        },
        __wbg_tagName_f764922f7d763584: function(arg0, arg1) {
            const ret = arg1.tagName;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_tangentialPressure_55610d0a052445d6: function(arg0) {
            const ret = arg0.tangentialPressure;
            return ret;
        },
        __wbg_targetTouches_55d7e91b9ddab16f: function(arg0) {
            const ret = arg0.targetTouches;
            return ret;
        },
        __wbg_target_3e80dc66a5936e87: function(arg0) {
            const ret = arg0.target;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_textContent_d7e4a707af82657f: function(arg0, arg1) {
            const ret = arg1.textContent;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_text_b00323a194b10fa8: function() { return handleError(function (arg0) {
            const ret = arg0.text();
            return ret;
        }, arguments); },
        __wbg_then_7a850dae4493f353: function(arg0, arg1, arg2) {
            const ret = arg0.then(arg1, arg2);
            return ret;
        },
        __wbg_then_b830475380919203: function(arg0, arg1) {
            const ret = arg0.then(arg1);
            return ret;
        },
        __wbg_tiltX_1ce87af3e8da3cde: function(arg0) {
            const ret = arg0.tiltX;
            return ret;
        },
        __wbg_tiltY_0479d3e1658b4b17: function(arg0) {
            const ret = arg0.tiltY;
            return ret;
        },
        __wbg_time_cc91f0f6671cd512: function(arg0) {
            const ret = arg0.time;
            return ret;
        },
        __wbg_title_533cbb7b090d0ad1: function(arg0, arg1) {
            const ret = arg1.title;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_top_319e8c7a40daacf1: function(arg0) {
            const ret = arg0.top;
            return ret;
        },
        __wbg_touches_fcb578b82332de47: function(arg0) {
            const ret = arg0.touches;
            return ret;
        },
        __wbg_twist_1a2feaa75e84cd1e: function(arg0) {
            const ret = arg0.twist;
            return ret;
        },
        __wbg_type_9a9a7042e88baa94: function(arg0, arg1) {
            const ret = arg1.type;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_type_de0ef83ccf892533: function(arg0, arg1) {
            const ret = arg1.type;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_type_e9e1e62b9a2c4682: function(arg0, arg1) {
            const ret = arg1.type;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_update_memory_4060a0ab30633318: function(arg0, arg1) {
            arg0.update_memory(arg1);
        },
        __wbg_value_000080a4486e3ed3: function(arg0, arg1) {
            const ret = arg1.value;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_value_8215ae6d5fec3d34: function(arg0, arg1) {
            const ret = arg1.value;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_value_88dbf3c61c816e23: function(arg0, arg1) {
            const ret = arg1.value;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_value_b50e7a14dbf411a1: function(arg0) {
            const ret = arg0.value;
            return ret;
        },
        __wbg_value_c227f843d21da141: function(arg0) {
            const ret = arg0.value;
            return ret;
        },
        __wbg_width_66ae60ec9345f56c: function(arg0) {
            const ret = arg0.width;
            return ret;
        },
        __wbg_width_71b671f940e3eb8c: function(arg0) {
            const ret = arg0.width;
            return ret;
        },
        __wbg_width_8d6f7dacd4cd7c61: function(arg0) {
            const ret = arg0.width;
            return ret;
        },
        __wbg_x_67554e02a6b67f0e: function(arg0) {
            const ret = arg0.x;
            return ret;
        },
        __wbg_y_8c87002a78bcd08f: function(arg0) {
            const ret = arg0.y;
            return ret;
        },
        __wbindgen_generic_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [Externref], shim_idx: 59, ret: Result(Unit), inner_ret: Some(Result(Unit)) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__core_608f92abc48d28da___result__Result_____wasm_bindgen_20ccca07c9e9b7d4___JsError___true_);
            return ret;
        },
        __wbindgen_generic_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [Externref], shim_idx: 88, ret: Externref, inner_ret: Some(Externref) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__wasm_bindgen_20ccca07c9e9b7d4___JsValue__true_);
            return ret;
        },
        __wbindgen_generic_0000000000000003: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [Externref], shim_idx: 90, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue______true_);
            return ret;
        },
        __wbindgen_generic_0000000000000004: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [I32], shim_idx: 88, ret: NamedExternref("Promise<any>"), inner_ret: Some(NamedExternref("Promise<any>")) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__wasm_bindgen_20ccca07c9e9b7d4___JsValue__true__5);
            return ret;
        },
        __wbindgen_generic_0000000000000005: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [NamedExternref("KeyboardEvent")], shim_idx: 145, ret: Unit, inner_ret: Some(Unit) }, mutable: false }) -> Externref`.
            const ret = makeClosure(arg0, arg1, wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___web_sys_c8ed461a97b049ba___features__gen_KeyboardEvent__KeyboardEvent______true_);
            return ret;
        },
        __wbindgen_generic_0000000000000006: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [Ref(NamedExternref("Event"))], shim_idx: 40, ret: Unit, inner_ret: Some(Unit) }, mutable: false }) -> Externref`.
            const ret = makeClosure(arg0, arg1, wasm_bindgen_20ccca07c9e9b7d4___convert__closures________invoke___web_sys_c8ed461a97b049ba___features__gen_Event__Event______true_);
            return ret;
        },
        __wbindgen_generic_0000000000000007: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [U32, U32, U32], shim_idx: 50, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__wasm_bindgen_20ccca07c9e9b7d4___JsValue__js_sys_c16d4f21c4e547e2___Set______true_);
            return ret;
        },
        __wbindgen_generic_0000000000000008: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [U32, U32, U32], shim_idx: 66, ret: NamedExternref("Promise<any>"), inner_ret: Some(NamedExternref("Promise<any>")) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__u32__js_sys_c16d4f21c4e547e2___Array__wasm_bindgen_20ccca07c9e9b7d4___JsValue__true_);
            return ret;
        },
        __wbindgen_generic_0000000000000009: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [U32, U32], shim_idx: 71, ret: NamedExternref("Promise<any>"), inner_ret: Some(NamedExternref("Promise<any>")) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__wasm_bindgen_20ccca07c9e9b7d4___JsValue__i32__true_);
            return ret;
        },
        __wbindgen_generic_000000000000000a: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [], shim_idx: 33, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke_______true_);
            return ret;
        },
        __wbindgen_generic_000000000000000b: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [], shim_idx: 96, ret: NamedExternref("Promise<any>"), inner_ret: Some(NamedExternref("Promise<any>")) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___js_sys_c16d4f21c4e547e2___Promise__true_);
            return ret;
        },
        __wbindgen_generic_000000000000000c: function(arg0) {
            // Cast intrinsic for `F64 -> Externref`.
            const ret = arg0;
            return ret;
        },
        __wbindgen_generic_000000000000000d: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./ksee_page_bg.js": import0,
    };
}

function wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke_______true_(arg0, arg1) {
    wasm.wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke_______true_(arg0, arg1);
}

function wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___js_sys_c16d4f21c4e547e2___Promise__true_(arg0, arg1) {
    const ret = wasm.wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___js_sys_c16d4f21c4e547e2___Promise__true_(arg0, arg1);
    return ret;
}

function wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue______true_(arg0, arg1, arg2) {
    wasm.wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue______true_(arg0, arg1, arg2);
}

function wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___web_sys_c8ed461a97b049ba___features__gen_KeyboardEvent__KeyboardEvent______true_(arg0, arg1, arg2) {
    wasm.wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___web_sys_c8ed461a97b049ba___features__gen_KeyboardEvent__KeyboardEvent______true_(arg0, arg1, arg2);
}

function wasm_bindgen_20ccca07c9e9b7d4___convert__closures________invoke___web_sys_c8ed461a97b049ba___features__gen_Event__Event______true_(arg0, arg1, arg2) {
    wasm.wasm_bindgen_20ccca07c9e9b7d4___convert__closures________invoke___web_sys_c8ed461a97b049ba___features__gen_Event__Event______true_(arg0, arg1, arg2);
}

function wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__wasm_bindgen_20ccca07c9e9b7d4___JsValue__true_(arg0, arg1, arg2) {
    const ret = wasm.wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__wasm_bindgen_20ccca07c9e9b7d4___JsValue__true_(arg0, arg1, arg2);
    return ret;
}

function wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__core_608f92abc48d28da___result__Result_____wasm_bindgen_20ccca07c9e9b7d4___JsError___true_(arg0, arg1, arg2) {
    const ret = wasm.wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__core_608f92abc48d28da___result__Result_____wasm_bindgen_20ccca07c9e9b7d4___JsError___true_(arg0, arg1, arg2);
    if (ret[1]) {
        throw takeFromExternrefTable0(ret[0]);
    }
}

function wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___js_sys_c16d4f21c4e547e2___Function_fn_wasm_bindgen_20ccca07c9e9b7d4___JsValue_____wasm_bindgen_20ccca07c9e9b7d4___sys__Undefined___js_sys_c16d4f21c4e547e2___Function_fn_wasm_bindgen_20ccca07c9e9b7d4___JsValue_____wasm_bindgen_20ccca07c9e9b7d4___sys__Undefined_______true_(arg0, arg1, arg2, arg3) {
    wasm.wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___js_sys_c16d4f21c4e547e2___Function_fn_wasm_bindgen_20ccca07c9e9b7d4___JsValue_____wasm_bindgen_20ccca07c9e9b7d4___sys__Undefined___js_sys_c16d4f21c4e547e2___Function_fn_wasm_bindgen_20ccca07c9e9b7d4___JsValue_____wasm_bindgen_20ccca07c9e9b7d4___sys__Undefined_______true_(arg0, arg1, arg2, arg3);
}

function wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__wasm_bindgen_20ccca07c9e9b7d4___JsValue__true__5(arg0, arg1, arg2) {
    const ret = wasm.wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__wasm_bindgen_20ccca07c9e9b7d4___JsValue__true__5(arg0, arg1, arg2);
    return ret;
}

function wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__wasm_bindgen_20ccca07c9e9b7d4___JsValue__i32__true_(arg0, arg1, arg2, arg3) {
    const ret = wasm.wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__wasm_bindgen_20ccca07c9e9b7d4___JsValue__i32__true_(arg0, arg1, arg2, arg3);
    return ret;
}

function wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__wasm_bindgen_20ccca07c9e9b7d4___JsValue__js_sys_c16d4f21c4e547e2___Set______true_(arg0, arg1, arg2, arg3, arg4) {
    wasm.wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__wasm_bindgen_20ccca07c9e9b7d4___JsValue__js_sys_c16d4f21c4e547e2___Set______true_(arg0, arg1, arg2, arg3, arg4);
}

function wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__u32__js_sys_c16d4f21c4e547e2___Array__wasm_bindgen_20ccca07c9e9b7d4___JsValue__true_(arg0, arg1, arg2, arg3, arg4) {
    const ret = wasm.wasm_bindgen_20ccca07c9e9b7d4___convert__closures_____invoke___wasm_bindgen_20ccca07c9e9b7d4___JsValue__u32__js_sys_c16d4f21c4e547e2___Array__wasm_bindgen_20ccca07c9e9b7d4___JsValue__true_(arg0, arg1, arg2, arg3, arg4);
    return ret;
}


const __wbindgen_enum_ScrollBehavior = ["auto", "instant", "smooth"];


const __wbindgen_enum_ScrollLogicalPosition = ["start", "center", "end", "nearest"];

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

const CLOSURE_DTORS = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(state => wasm.__wbindgen_destroy_closure(state.a, state.b));

function debugString(val) {
    // primitive types
    const type = typeof val;
    if (type == 'number' || type == 'boolean' || val == null) {
        return  `${val}`;
    }
    if (type == 'string') {
        return `"${val}"`;
    }
    if (type == 'symbol') {
        const description = val.description;
        if (description == null) {
            return 'Symbol';
        } else {
            return `Symbol(${description})`;
        }
    }
    if (type == 'function') {
        const name = val.name;
        if (typeof name == 'string' && name.length > 0) {
            return `Function(${name})`;
        } else {
            return 'Function';
        }
    }
    // objects
    if (Array.isArray(val)) {
        const length = val.length;
        let debug = '[';
        if (length > 0) {
            debug += debugString(val[0]);
        }
        for(let i = 1; i < length; i++) {
            debug += ', ' + debugString(val[i]);
        }
        debug += ']';
        return debug;
    }
    // Test for built-in
    const builtInMatches = /\[object ([^\]]+)\]/.exec(toString.call(val));
    let className;
    if (builtInMatches && builtInMatches.length > 1) {
        className = builtInMatches[1];
    } else {
        // Failed to match the standard '[object ClassName]'
        return toString.call(val);
    }
    if (className == 'Object') {
        // we're a user defined class or Object
        // JSON.stringify avoids problems with cycles, and is generally much
        // easier than looping through ownProperties of `val`.
        try {
            return 'Object(' + JSON.stringify(val) + ')';
        } catch (_) {
            return 'Object';
        }
    }
    // errors
    if (val instanceof Error) {
        return `${val.name}: ${val.message}\n${val.stack}`;
    }
    // TODO we could test for more things here, like `Set`s and `Map`s.
    return className;
}

function getArrayJsValueFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    const mem = getDataViewMemory0();
    const result = [];
    for (let i = ptr; i < ptr + 4 * len; i += 4) {
        result.push(wasm.__wbindgen_externrefs.get(mem.getUint32(i, true)));
    }
    wasm.__externref_drop_slice(ptr, len);
    return result;
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function makeClosure(arg0, arg1, f) {
    const state = { a: arg0, b: arg1, cnt: 1 };
    const real = (...args) => {

        // First up with a closure we increment the internal reference
        // count. This ensures that the Rust closure environment won't
        // be deallocated while we're invoking it.
        state.cnt++;
        try {
            return f(state.a, state.b, ...args);
        } finally {
            real._wbg_cb_unref();
        }
    };
    real._wbg_cb_unref = () => {
        if (--state.cnt === 0) {
            wasm.__wbindgen_destroy_closure(state.a, state.b);
            state.a = 0;
            CLOSURE_DTORS.unregister(state);
        }
    };
    CLOSURE_DTORS.register(real, state, state);
    return real;
}

function makeMutClosure(arg0, arg1, f) {
    const state = { a: arg0, b: arg1, cnt: 1 };
    const real = (...args) => {

        // First up with a closure we increment the internal reference
        // count. This ensures that the Rust closure environment won't
        // be deallocated while we're invoking it.
        state.cnt++;
        const a = state.a;
        state.a = 0;
        try {
            return f(a, state.b, ...args);
        } finally {
            state.a = a;
            real._wbg_cb_unref();
        }
    };
    real._wbg_cb_unref = () => {
        if (--state.cnt === 0) {
            wasm.__wbindgen_destroy_closure(state.a, state.b);
            state.a = 0;
            CLOSURE_DTORS.unregister(state);
        }
    };
    CLOSURE_DTORS.register(real, state, state);
    return real;
}

function passArrayJsValueToWasm0(array, malloc) {
    const ptr = malloc(array.length * 4, 4) >>> 0;
    for (let i = 0; i < array.length; i++) {
        const add = addToExternrefTable0(array[i]);
        getDataViewMemory0().setUint32(ptr + 4 * i, add, true);
    }
    WASM_VECTOR_LEN = array.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (!module.ok) {
            throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
        }

        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }


    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
