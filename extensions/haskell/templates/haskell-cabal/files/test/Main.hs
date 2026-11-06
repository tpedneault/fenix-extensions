-- | The tests, on base alone: each a name, what came of it and what
-- should have. Add hspec or tasty to the test suite's build-depends for
-- more.
module Main (main) where

import Control.Monad (unless)
import System.Exit (exitFailure)

import {{name_pascal}} (greet)

main :: IO ()
main = do
  let results =
        [ ("one name", greet ["Ada"], Right "Hello, Ada!")
        , ("two names", greet ["Ada", "Grace"], Right "Hello, Ada and Grace!")
        , ("no names", greet [], Left "whom to greet?")
        ]
      failed = [name | (name, got, want) <- results, got /= want]
  mapM_ (\name -> putStrLn ("FAIL " ++ name)) failed
  unless (null failed) exitFailure
  putStrLn (show (length results) ++ " tests passed")
