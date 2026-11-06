module Main (main) where

import System.Environment (getArgs)
import System.Exit (exitWith, ExitCode (..))
import System.IO (hPutStrLn, stderr)

import {{name_pascal}} (greet)

main :: IO ()
main = do
  args <- getArgs
  case greet args of
    Right line -> putStrLn line
    Left why -> do
      hPutStrLn stderr ("{{name_kebab}}: " ++ why)
      exitWith (ExitFailure 2)
