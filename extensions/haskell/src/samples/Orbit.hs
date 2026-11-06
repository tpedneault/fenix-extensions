{-# LANGUAGE OverloadedStrings #-}
-- | Orbits, kept by name.
module Orbit
  ( Orbit (..)
  , Kind (..)
  , describe
  , main
  ) where

import qualified Data.Map.Strict as Map
import Data.List (sortOn)
import Control.Exception (throwIO)

-- | An orbit, by its period.
data Orbit = Orbit
  { name :: String
  , period :: Double
  , crew :: Int
  }
  deriving (Show, Eq)

data Kind = Leo | Meo | Geo
  deriving (Show, Eq, Ord, Enum, Bounded)

newtype Registry = Registry (Map.Map String Orbit)

class Described a where
  describe :: a -> String

instance Described Orbit where
  describe o = name o ++ " every " ++ show (period o) ++ "s\n"

kind :: Double -> Kind
kind altitude
  | altitude < 2000 = Leo
  | altitude < 35786 = Meo
  | otherwise = Geo

maxCrew :: Int
maxCrew = 7

register :: Orbit -> Registry -> Either String Registry
register o (Registry known) =
  case Map.lookup (name o) known of
    Just _ -> Left "already known"
    Nothing
      | crew o > maxCrew -> Left "too many aboard"
      | otherwise -> Right (Registry (Map.insert (name o) o known))

main :: IO ()
main = do
  let iss = Orbit {name = "ISS", period = 5520, crew = 3}
      ratio = 1.5e3 :: Double
  if crew iss > 0
    then putStrLn (describe iss)
    else throwIO (userError "empty")
  mapM_ print (sortOn period [iss])
  print (map (\x -> x * 2) [1, 2, 3 :: Int], ratio, True, 'x')
  where
    helper = 'y'
