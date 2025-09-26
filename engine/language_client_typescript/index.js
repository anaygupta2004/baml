"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __exportStar = (this && this.__exportStar) || function(m, exports) {
    for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports, p)) __createBinding(exports, m, p);
};
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.BamlCtxManager = exports.BamlStream = exports.TraceStats = exports.Timing = exports.StreamTiming = exports.SSEResponse = exports.HTTPResponse = exports.HTTPRequest = exports.Usage = exports.LlmStreamCall = exports.LlmCall = exports.FunctionLog = exports.Collector = exports.BamlLogEvent = exports.ClientRegistry = exports.invoke_runtime_cli = exports.Video = exports.Pdf = exports.Audio = exports.Image = exports.FunctionResultStream = exports.FunctionResult = exports.BamlRuntime = void 0;
__exportStar(require("./safe_imports"), exports);
__exportStar(require("./errors"), exports);
__exportStar(require("./logging"), exports);
// Import native implementations
const native = __importStar(require("../native"));
// Export native implementations for Node.js environment
exports.BamlRuntime = native.BamlRuntime;
exports.FunctionResult = native.FunctionResult;
exports.FunctionResultStream = native.FunctionResultStream;
exports.Image = native.BamlImage;
exports.Audio = native.BamlAudio;
exports.Pdf = native.BamlPdf;
exports.Video = native.BamlVideo;
exports.invoke_runtime_cli = native.invoke_runtime_cli;
exports.ClientRegistry = native.ClientRegistry;
exports.BamlLogEvent = native.BamlLogEvent;
exports.Collector = native.Collector;
exports.FunctionLog = native.FunctionLog;
exports.LlmCall = native.LlmCall;
exports.LlmStreamCall = native.LlmStreamCall;
exports.Usage = native.Usage;
exports.HTTPRequest = native.HTTPRequest;
exports.HTTPResponse = native.HTTPResponse;
exports.SSEResponse = native.SSEResponse;
exports.StreamTiming = native.StreamTiming;
exports.Timing = native.Timing;
exports.TraceStats = native.TraceStats;
var stream_1 = require("./stream");
Object.defineProperty(exports, "BamlStream", { enumerable: true, get: function () { return stream_1.BamlStream; } });
var async_context_vars_1 = require("./async_context_vars");
Object.defineProperty(exports, "BamlCtxManager", { enumerable: true, get: function () { return async_context_vars_1.BamlCtxManager; } });
// Detect if we're in a Node.js environment
const isNode = typeof process !== 'undefined' && process.versions != null && process.versions.node != null;
if (!isNode) {
    const browserError = (name) => {
        throw new Error(`Cannot import ${name} from '@boundaryml/baml' in browser environment. Please import from '@boundaryml/baml/browser' instead.`);
    };
    // Provide helpful error messages for browser imports
    Object.defineProperty(exports, 'Image', {
        get: () => browserError('Image'),
        enumerable: true,
    });
    Object.defineProperty(exports, 'Audio', {
        get: () => browserError('Audio'),
        enumerable: true,
    });
    Object.defineProperty(exports, 'Pdf', {
        get: () => browserError('Pdf'),
        enumerable: true,
    });
    Object.defineProperty(exports, 'Video', {
        get: () => browserError('Video'),
        enumerable: true,
    });
}
