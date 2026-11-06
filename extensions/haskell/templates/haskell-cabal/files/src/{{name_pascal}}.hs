-- | What {{name_kebab}} does: greets whoever it's told to.
module {{name_pascal}}
  ( greet
  ) where

import Data.List (intercalate)

-- | A greeting for each name, together: @greet ["Ada", "Grace"]@ is
-- @"Hello, Ada and Grace!"@.
greet :: [String] -> Either String String
greet [] = Left "whom to greet?"
greet names = Right ("Hello, " ++ intercalate " and " names ++ "!")
