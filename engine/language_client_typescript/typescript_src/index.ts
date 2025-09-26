export * from './safe_imports'

export * from './errors'

export * from './logging'

// Import native implementations
import * as native from '../native'

// Export native implementations for Node.js environment
export const BamlRuntime = native.BamlRuntime
export const FunctionResult = native.FunctionResult
export const FunctionResultStream = native.FunctionResultStream
export const Image = native.BamlImage
export const Audio = native.BamlAudio
export const Pdf = native.BamlPdf
export const Video = native.BamlVideo
export const invoke_runtime_cli = native.invoke_runtime_cli
export const ClientRegistry = native.ClientRegistry
export const BamlLogEvent = native.BamlLogEvent
export const Collector = native.Collector
export const FunctionLog = native.FunctionLog
export const LlmCall = native.LlmCall
export const LlmStreamCall = native.LlmStreamCall
export const Usage = native.Usage
export const HTTPRequest = native.HTTPRequest
export const HTTPResponse = native.HTTPResponse
export const SSEResponse = native.SSEResponse
export const StreamTiming = native.StreamTiming
export const Timing = native.Timing
export const TraceStats = native.TraceStats

export { BamlStream } from './stream'
export { BamlCtxManager } from './async_context_vars'

// Detect if we're in a Node.js environment
const isNode = typeof process !== 'undefined' && process.versions != null && process.versions.node != null

if (!isNode) {
  const browserError = (name: string) => {
    throw new Error(
      `Cannot import ${name} from '@boundaryml/baml' in browser environment. Please import from '@boundaryml/baml/browser' instead.`,
    )
  }

  // Provide helpful error messages for browser imports
  Object.defineProperty(exports, 'Image', {
    get: () => browserError('Image'),
    enumerable: true,
  })

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
