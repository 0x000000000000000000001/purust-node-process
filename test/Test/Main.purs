module Test.Main where

import Prelude

import Data.Array (length, (!!))
import Data.Maybe (Maybe(..), isJust, isNothing)
import Data.Nullable (toNullable)
import Data.Posix (Pid(..))
import Data.Posix.Signal (Signal(..))
import Effect (Effect)
import Effect.Aff (Milliseconds(..), delay, launchAff_)
import Effect.Class (liftEffect)
import Effect.Console (log)
import Effect.Exception (throw)
import Effect.Ref as Ref
import Foreign.Object as Object
import Node.EventEmitter (on_)
import Node.Process as P
import Test.Assert (assert, assertEqual)

-- | The runner drives extra lifecycle scenarios by passing the scenario as the
-- | first user argument. `argv` is `[exec, exec, ...]`, like `node script.js`.
main :: Effect Unit
main = do
  args <- P.argv
  case args !! 2 of
    Just "--child-exit" -> childExit
    Just "--child-set-exit-code" -> childSetExitCode
    Just "--child-capture" -> childCapture
    Just "--child-throw" -> childThrow
    _ -> suite

-- | The `exit` event sees the code and the process terminates with it.
childExit :: Effect Unit
childExit = do
  on_ P.exitH (\code -> log ("exit-event: " <> show code)) P.process
  P.setExitCode 42
  P.exit

-- | `setExitCode` is honoured when the program finishes on its own.
childSetExitCode :: Effect Unit
childSetExitCode = do
  P.setExitCode 7
  log "natural-exit"

-- | With a capture callback installed, an uncaught exception runs it; the
-- | callback picks the final status.
childCapture :: Effect Unit
childCapture = do
  P.setUncaughtExceptionCaptureCallback do
    log "capture: uncaught"
    P.exit' 7
  throw "uncaught-boom"

-- | Without a capture callback, an uncaught exception fails the process.
childThrow :: Effect Unit
childThrow = throw "uncaught-boom"

suite :: Effect Unit
suite = do
  -- Identity and platform
  assert (P.pid > Pid 0)
  assert (P.ppid >= Pid 0)
  assert (P.version /= "")
  assert (isJust P.platform)
  assertEqual { expected: 9229, actual: P.debugPort }

  -- Arguments and executable
  args <- P.argv
  assert (length args >= 1)
  argv0 <- P.argv0
  assert (argv0 /= "")
  exec <- P.execPath
  assert (exec /= "")
  execArgs <- P.execArgv
  assertEqual { expected: [], actual: execArgs }

  -- Working directory
  start <- P.cwd
  assert (start /= "")
  P.chdir "/"
  atRoot <- P.cwd
  assertEqual { expected: "/", actual: atRoot }
  P.chdir start
  back <- P.cwd
  assertEqual { expected: start, actual: back }

  -- Environment
  env <- P.getEnv
  assert (Object.size env > 0)
  P.setEnv "PURUST_NODE_PROCESS_TEST" "hello"
  value <- P.lookupEnv "PURUST_NODE_PROCESS_TEST"
  assertEqual { expected: Just "hello", actual: value }
  P.unsetEnv "PURUST_NODE_PROCESS_TEST"
  removed <- P.lookupEnv "PURUST_NODE_PROCESS_TEST"
  assertEqual { expected: Nothing, actual: removed }

  -- Exit code
  P.setExitCode 5
  code <- P.getExitCode
  assertEqual { expected: Just 5, actual: code }
  P.setExitCode 0

  -- User and group ids
  gid <- P.getGid
  uid <- P.getUid
  assert (isJust gid)
  assert (isJust uid)

  -- Uncaught exception callback bookkeeping
  initial <- P.hasUncaughtExceptionCaptureCallback
  assertEqual { expected: false, actual: initial }
  P.setUncaughtExceptionCaptureCallback (pure unit)
  installed <- P.hasUncaughtExceptionCaptureCallback
  assertEqual { expected: true, actual: installed }
  P.clearUncaughtExceptionCaptureCallback
  cleared <- P.hasUncaughtExceptionCaptureCallback
  assertEqual { expected: false, actual: cleared }

  -- Signals: 0 probes the process, SIGCONT is harmless on self.
  P.killInt P.pid 0
  P.killStr P.pid "SIGCONT"

  -- Usage records
  memory <- P.memoryUsage
  assert (memory.rss > 0)
  rss <- P.memoryUsageRss
  assert (rss > 0)
  cpu <- P.cpuUsageToRecord <$> P.cpuUsage
  assert (cpu.user >= 0)
  assert (cpu.system >= 0)
  diff <- P.cpuUsageToRecord <$> (P.cpuUsageDiff =<< P.cpuUsage)
  assert (diff.user >= 0)
  usage <- P.resourceUsage
  assert (usage.maxRSS >= 0)
  assert (usage.minorPageFault >= 0)

  -- No IPC channel is connected: `send` reports `false` for every variant and
  -- never throws, and the callbacks are not invoked. Real IPC communication is
  -- not ported; these are the observable guarantees without a channel.
  assert (isNothing P.channelRef)
  assert (isNothing P.channelUnref)
  assert (isNothing P.disconnect)
  connected <- P.connected
  assertEqual { expected: false, actual: connected }
  sent <- P.unsafeSend { hello: "world" } (toNullable Nothing)
  assertEqual { expected: false, actual: sent }
  sentOpts <- P.unsafeSendOpts { hello: "world" } (toNullable Nothing) { keepAlive: false }
  assertEqual { expected: false, actual: sentOpts }
  sendCbErrors <- Ref.new 0
  sentCb <- P.unsafeSendCb { hello: "world" } (toNullable Nothing) \_ ->
    Ref.modify_ (_ + 1) sendCbErrors
  assertEqual { expected: false, actual: sentCb }
  sentOptsCb <- P.unsafeSendOptsCb { hello: "world" } (toNullable Nothing) { keepAlive: false } \_ ->
    Ref.modify_ (_ + 1) sendCbErrors
  assertEqual { expected: false, actual: sentOptsCb }
  callbackErrors <- Ref.read sendCbErrors
  assertEqual { expected: 0, actual: callbackErrors }
  config <- P.config
  let _ = config

  -- TTY probes and native abort handle
  let _ = P.stdinIsTTY
  let _ = P.stdoutIsTTY
  let _ = P.stderrIsTTY
  assert (isJust P.abort)

  -- Title
  P.setTitle "purust-node-process-test"
  title <- P.getTitle
  assertEqual { expected: "purust-node-process-test", actual: title }

  -- Uptime
  uptime <- P.uptime
  assert (uptime >= 0.0)

  -- Event handles register on the process emitter.
  on_ P.beforeExitH (\_ -> pure unit) P.process
  on_ P.disconnectH (pure unit) P.process
  on_ P.exitH (\_ -> pure unit) P.process
  on_ P.messageH (\_ _ -> pure unit) P.process
  on_ P.rejectionHandledH (\_ -> pure unit) P.process
  on_ P.uncaughtExceptionH (\_ _ -> pure unit) P.process
  on_ P.unhandledRejectionH (\_ _ -> pure unit) P.process
  on_ (P.mkSignalH SIGCONT) (pure unit) P.process
  on_ (P.mkSignalH' "cont") (pure unit) P.process
  on_ P.warningH (\_ -> pure unit) P.process
  on_ P.workerH (\_ -> pure unit) P.process

  -- `nextTick` defers to the microtask queue, `nextTick'` also forwards its
  -- argument. Both run exactly once, in scheduling order, and never inline.
  tickOrder <- Ref.new ([] :: Array String)
  tickArgs <- Ref.new Nothing
  tickCalls <- Ref.new 0
  P.nextTick do
    Ref.modify_ (_ <> [ "first" ]) tickOrder
  P.nextTick do
    Ref.modify_ (_ <> [ "second" ]) tickOrder
    Ref.modify_ (_ + 1) tickCalls
  P.nextTick' (\arg -> do
      Ref.write (Just arg.name) tickArgs
      Ref.modify_ (_ <> [ "argument" ]) tickOrder
    ) { name: "purust" }
  immediate <- Ref.read tickOrder
  assertEqual { expected: [], actual: immediate }
  launchAff_ do
    delay (Milliseconds 10.0)
    order <- liftEffect (Ref.read tickOrder)
    liftEffect $ assertEqual { expected: [ "first", "second", "argument" ], actual: order }
    argument <- liftEffect (Ref.read tickArgs)
    liftEffect $ assertEqual { expected: Just "purust", actual: argument }
    calls <- liftEffect (Ref.read tickCalls)
    liftEffect $ assertEqual { expected: 1, actual: calls }
    liftEffect $ log "Tests passed"
