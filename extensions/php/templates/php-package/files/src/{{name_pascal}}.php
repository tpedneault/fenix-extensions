<?php

declare(strict_types=1);

namespace {{namespace}}\{{name_pascal}};

/**
 * Reverses strings, a character at a time: "héllo" is "olléh".
 */
final class {{name_pascal}}
{
    public static function reverse(string $text): string
    {
        return implode('', array_reverse(mb_str_split($text)));
    }
}
