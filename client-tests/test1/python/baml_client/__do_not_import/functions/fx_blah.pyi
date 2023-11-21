# This file is generated by the BAML compiler.
# Do not edit this file directly.
# Instead, edit the BAML files and recompile.

# ruff: noqa: E501,F401
# flake8: noqa: E501,F401
# pylint: disable=unused-import,line-too-long
# fmt: off

from typing import Protocol, runtime_checkable


import typing

import pytest

ImplName = typing.Literal["v1", "v2"]

T = typing.TypeVar("T", bound=typing.Callable[..., typing.Any])
CLS = typing.TypeVar("CLS", bound=type)


IBlahOutput = str

@runtime_checkable
class IBlah(Protocol):
    """
    This is the interface for a function.

    Args:
        arg: str

    Returns:
        str
    """

    async def __call__(self, arg: str, /) -> str:
        ...


class BAMLBlahImpl:
    async def run(self, arg: str, /) -> str:
        ...

class IBAMLBlah:
    def register_impl(
        self, name: ImplName
    ) -> typing.Callable[[IBlah], IBlah]:
        ...

    def get_impl(self, name: ImplName) -> BAMLBlahImpl:
        ...

    @typing.overload
    def test(self, test_function: T) -> T:
        """
        Provides a pytest.mark.parametrize decorator to facilitate testing different implementations of
        the BlahInterface.

        Args:
            test_function : T
                The test function to be decorated.

        Usage:
            ```python
            # All implementations will be tested.

            @baml.Blah.test
            def test_logic(BlahImpl: IBlah) -> None:
                result = await BlahImpl(...)
            ```
        """
        ...

    @typing.overload
    def test(self, *, exclude_impl: typing.Iterable[ImplName]) -> pytest.MarkDecorator:
        """
        Provides a pytest.mark.parametrize decorator to facilitate testing different implementations of
        the BlahInterface.

        Args:
            exclude_impl : Iterable[ImplName]
                The names of the implementations to exclude from testing.

        Usage:
            ```python
            # All implementations except "v1" will be tested.

            @baml.Blah.test(exclude_impl=["v1"])
            def test_logic(BlahImpl: IBlah) -> None:
                result = await BlahImpl(...)
            ```
        """
        ...

    @typing.overload
    def test(self, test_class: typing.Type[CLS]) -> typing.Type[CLS]:
        """
        Provides a pytest.mark.parametrize decorator to facilitate testing different implementations of
        the BlahInterface.

        Args:
            test_class : Type[CLS]
                The test class to be decorated.

        Usage:
        ```python
        # All implementations will be tested in every test method.

        @baml.Blah.test
        class TestClass:
            def test_a(self, BlahImpl: IBlah) -> None:
                ...
            def test_b(self, BlahImpl: IBlah) -> None:
                ...
        ```
        """
        ...

BAMLBlah: IBAMLBlah
