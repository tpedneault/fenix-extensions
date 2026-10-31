<?php

declare(strict_types=1);

namespace {{namespace}}\{{name_pascal}}\Tests;

use {{namespace}}\{{name_pascal}}\{{name_pascal}};
use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\Attributes\Test;
use PHPUnit\Framework\TestCase;

final class {{name_pascal}}Test extends TestCase
{
    /** @return iterable<string, array{string, string}> */
    public static function words(): iterable
    {
        yield 'empty' => ['', ''];
        yield 'a word' => ['stressed', 'desserts'];
        yield 'accents' => ['héllo', 'olléh'];
    }

    #[Test]
    #[DataProvider('words')]
    public function it_reverses(string $text, string $reversed): void
    {
        self::assertSame($reversed, {{name_pascal}}::reverse($text));
    }
}
