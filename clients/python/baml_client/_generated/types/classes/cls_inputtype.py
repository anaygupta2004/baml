# This file is generated by the BAML compiler.
# Do not edit this file directly.
# Instead, edit the BAML files and recompile.
#
# BAML version: 0.0.1
# Generated Date: 2023-10-30 01:08:44.810818 -07:00
# Generated by: vbv

from ...._impl.deserializer import register_deserializer
from pydantic import BaseModel


@register_deserializer()
class InputType(BaseModel):
    a: InputType2
    b: bool
