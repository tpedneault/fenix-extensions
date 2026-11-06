module Hints where

describe :: Int -> String
describe n = "crew: " ++ (show n)

shout :: String -> IO ()
shout s = putStrLn (show s)

twice :: [Int] -> [Int]
twice xs = map (\x -> x * 2) xs
