#!/usr/bin/env python3
"""
exeiconextract.py - Extract icon from Windows EXE to PNG

Based on KDE's thumbnailer code for Windows executables.
Parses the PE file format directly to extract icons correctly.

Usage: python3 exeiconextract.py <input.exe> <output.png>
"""

import struct
import sys
from typing import List, Dict, Optional, Tuple


class IconInfo:
    """Information about an extracted icon"""
    def __init__(self, width: int, height: int, bpp: int, data: bytes):
        self.width = width
        self.height = height
        self.bpp = bpp
        self.data = data


def extract_icon_from_exe(data: bytes) -> List[IconInfo]:
    """
    Extract all icons from a Windows PE executable.
    Returns a list of IconInfo objects.
    """
    # Parse DOS header
    if len(data) < 64 or data[0:2] != b'MZ':
        return []

    pe_offset = struct.unpack('<I', data[60:64])[0]

    # Check PE signature
    if len(data) < pe_offset + 4 or data[pe_offset:pe_offset+4] != b'PE\0\0':
        return []

    # Get PE type (PE32 vs PE32+)
    num_sections = struct.unpack('<H', data[pe_offset+6:pe_offset+8])[0]
    optional_header_size = struct.unpack('<H', data[pe_offset+20:pe_offset+22])[0]
    opt_magic = struct.unpack('<H', data[pe_offset+24:pe_offset+26])[0]

    is_pe32_plus = (opt_magic == 0x020b)

    # Build section lookup table
    section_table_offset = pe_offset + 24 + optional_header_size
    sections: List[Tuple[int, int, int]] = []  # (VA, size, raw_offset)

    for i in range(num_sections):
        offset = section_table_offset + i * 40
        if offset + 40 > len(data):
            break
        virtual_address = struct.unpack('<I', data[offset+12:offset+16])[0]
        size_of_raw_data = struct.unpack('<I', data[offset+16:offset+20])[0]
        pointer_to_raw_data = struct.unpack('<I', data[offset+20:offset+24])[0]
        sections.append((virtual_address, size_of_raw_data, pointer_to_raw_data))

    def rva_to_offset(rva: int) -> int:
        """Convert Relative Virtual Address to file offset"""
        for va, size, raw in sections:
            if va <= rva < va + size:
                return rva - va + raw
        return -1

    # Find resource directory
    if is_pe32_plus:
        data_dir_offset = pe_offset + 24 + 112 + 2 * 8  # Resource is index 2
    else:
        data_dir_offset = pe_offset + 24 + 96 + 2 * 8

    if data_dir_offset + 8 > len(data):
        return []

    resource_va = struct.unpack('<I', data[data_dir_offset:data_dir_offset+4])[0]
    if resource_va == 0:
        return []

    resource_offset = rva_to_offset(resource_va)
    if resource_offset < 0:
        return []

    SUBDIRECTORY_BIT = 0x80000000  # High bit for subdirectory flag

    # Function to read resource directory entries (like KDE's readResourceDataDirectoryEntry)
    def read_resource_directory_entries(base_offset: int) -> List[Tuple[int, int]]:
        """
        Read all directory entries.
        Returns list of (resourceId, offset) tuples.
        Like KDE, we read ALL entries (name and ID combined)
        """
        # Read header to get counts
        if base_offset + 16 > len(data):
            return []

        num_name_entries = struct.unpack('<H', data[base_offset + 12:base_offset + 14])[0]
        num_id_entries = struct.unpack('<H', data[base_offset + 14:base_offset + 16])[0]
        total_entries = num_name_entries + num_id_entries

        entries = []
        entries_start = base_offset + 16

        for i in range(total_entries):
            entry_offset = entries_start + i * 8
            if entry_offset + 8 > len(data):
                break

            # First uint32: resourceId (or name RVA for name entries)
            resource_id = struct.unpack('<I', data[entry_offset:entry_offset + 4])[0]
            # Second uint32: offset (with or without high bit)
            offset = struct.unpack('<I', data[entry_offset + 4:entry_offset + 8])[0]

            entries.append((resource_id, offset))

        return entries

    # Collect icon resources: resourceId -> (va, size)
    icon_resources: Dict[int, Tuple[int, int]] = {}
    group_icon_data_entry: Optional[Tuple[int, int]] = None  # (dataVA, dataSize)

    # Level 1: Resource types
    level1_entries = read_resource_directory_entries(resource_offset)

    for type_id, level2_offset_raw in level1_entries:
        # Skip if not a subdirectory (offset & 0x80000000 == 0)
        if level2_offset_raw & SUBDIRECTORY_BIT == 0:
            continue

        level2_abs = resource_offset + (level2_offset_raw & ~SUBDIRECTORY_BIT)
        if level2_abs + 16 > len(data):
            continue

        # We only care about type 3 (Icon) and type 14 (GroupIcon)
        if type_id not in (3, 14):
            continue

        # Level 2: Resource IDs (key values!)
        level2_entries = read_resource_directory_entries(level2_abs)

        for resource_id, level3_offset_raw in level2_entries:
            # Skip if not a subdirectory (no level 3 means no language selection)
            if level3_offset_raw & SUBDIRECTORY_BIT == 0:
                continue

            level3_abs = resource_offset + (level3_offset_raw & ~SUBDIRECTORY_BIT)
            if level3_abs + 16 > len(data):
                continue

            # Level 3: Language entries -> data entries
            level3_entries = read_resource_directory_entries(level3_abs)

            for language_id, data_offset_raw in level3_entries:
                # Skip if subdirectory bit is SET (we want data entries, not subdirs)
                if data_offset_raw & SUBDIRECTORY_BIT == SUBDIRECTORY_BIT:
                    continue

                data_entry_abs = resource_offset + data_offset_raw
                if data_entry_abs + 16 <= len(data):
                    data_va = struct.unpack('<I', data[data_entry_abs:data_entry_abs + 4])[0]
                    data_size = struct.unpack('<I', data[data_entry_abs + 4:data_entry_abs + 8])[0]

                    if type_id == 3:  # Icon
                        # Use resource_id from level 2 as the key (like KDE)
                        icon_resources[resource_id] = (data_va, data_size)
                    elif type_id == 14 and group_icon_data_entry is None:  # GroupIcon - take first
                        group_icon_data_entry = (data_va, data_size)

    if group_icon_data_entry is None or not icon_resources:
        return []

    group_va, group_size = group_icon_data_entry
    group_offset = rva_to_offset(group_va)
    if group_offset < 0:
        return []

    # Parse group icon directory structure
    if group_offset + 6 > len(data):
        return []

    type_val = struct.unpack('<H', data[group_offset + 2:group_offset + 4])[0]
    num_icons = struct.unpack('<H', data[group_offset + 4:group_offset + 6])[0]

    if type_val != 1 or num_icons == 0:
        return []

    icons: List[IconInfo] = []

    # Detect actual entry size from the data size
    # Standard is 16 bytes, but some EXEs use 14 bytes (packed)
    if group_size >= 6:
        entry_size = (group_size - 6) // num_icons
        if entry_size not in (14, 16):
            entry_size = 16  # fallback to standard
    else:
        entry_size = 16

    # Parse each icon entry in the group
    for i in range(num_icons):
        entry_offset = group_offset + 6 + i * entry_size
        if entry_offset + entry_size > len(data):
            break

        # For 14-byte format: w(1), h(1), color(1), reserved(1), planes(2), bpp(2), [corrupted size], id(2)
        # For 16-byte format: w(1), h(1), color(1), reserved(1), planes(2), bpp(2), size(4), id(2)
        if entry_size == 14:
            width = struct.unpack('B', data[entry_offset:entry_offset + 1])[0]
            height = struct.unpack('B', data[entry_offset + 1:entry_offset + 2])[0]
            color_count = struct.unpack('B', data[entry_offset + 2:entry_offset + 3])[0]
            # Size field is at offset 8 (4 bytes) but may be corrupted
            resource_id = struct.unpack('<H', data[entry_offset + 12:entry_offset + 14])[0]
        else:  # 16-byte format
            width = struct.unpack('B', data[entry_offset:entry_offset + 1])[0]
            height = struct.unpack('B', data[entry_offset + 1:entry_offset + 2])[0]
            color_count = struct.unpack('B', data[entry_offset + 2:entry_offset + 3])[0]
            planes = struct.unpack('<H', data[entry_offset + 4:entry_offset + 6])[0]
            bpp = struct.unpack('<H', data[entry_offset + 6:entry_offset + 8])[0]
            resource_id = struct.unpack('<H', data[entry_offset + 14:entry_offset + 16])[0]
            actual_bpp = bpp if bpp > 0 else 32
            planes = planes if planes > 0 else 1

        # Handle special values (0 means 256)
        # Note: group entry may have corrupted dimensions, fix them after extraction
        actual_width = 256 if width == 0 else width
        actual_height = 256 if height == 0 else height

        # Find the actual icon data using resourceId from group entry
        if resource_id in icon_resources:
            icon_va, icon_size = icon_resources[resource_id]
            icon_data_offset = rva_to_offset(icon_va)

            if icon_data_offset >= 0 and icon_data_offset + icon_size <= len(data):
                icon_data = data[icon_data_offset:icon_data_offset + icon_size]

                # Build a proper .ico file
                # .ico format: header (6 bytes) + entries (16 bytes each) + icon data
                ico_header = struct.pack('<HHH', 0, 1, 1)  # reserved, type (1=ico), count (1)
                ico_entry = struct.pack(
                    '<BBBBHHII',
                    width, height, color_count, 0,  # width, height, colorCount, reserved
                    1 if entry_size == 14 else planes,  # numPlanes
                    32 if entry_size == 14 else actual_bpp,  # bpp (use 32 for 14-byte corrupted entries)
                    icon_size, 22  # size, offset (header size + entry size)
                )

                full_ico = ico_header + ico_entry + icon_data
                icons.append(IconInfo(actual_width, actual_height, 32 if entry_size == 14 else actual_bpp, full_ico))

    return icons


def ico_to_png(ico_data: bytes, output_path: str) -> bool:
    """Convert .ico data to PNG using available tools"""
    # Try PIL (Pillow) first
    try:
        from PIL import Image
        import io
        img = Image.open(io.BytesIO(ico_data))
        img.save(output_path, 'PNG')
        return True
    except ImportError:
        pass
    except Exception:
        pass

    # Fallback: save as .ico and use ImageMagick
    try:
        import os
        temp_ico = output_path.rsplit('.', 1)[0] + '_temp.ico'
        with open(temp_ico, 'wb') as f:
            f.write(ico_data)

        ret = os.system(f'magick "{temp_ico}"[0] "{output_path}" 2>/dev/null')
        try:
            os.remove(temp_ico)
        except:
            pass
        return ret == 0
    except Exception:
        pass

    return False


def select_best_icon(icons: List[IconInfo]) -> Optional[IconInfo]:
    """Select the largest (highest quality) icon"""
    if not icons:
        return None
    return max(icons, key=lambda i: i.width * i.height * i.bpp)


def main():
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <input.exe> <output.png>")
        print(f"Example: {sys.argv[0]} game.exe icon.png")
        return 1

    input_exe = sys.argv[1]
    output_png = sys.argv[2]

    try:
        with open(input_exe, 'rb') as f:
            data = f.read()
    except Exception as e:
        print(f"Error reading {input_exe}: {e}", file=sys.stderr)
        return 1

    icons = extract_icon_from_exe(data)

    if not icons:
        print(f"Error: No icons found in {input_exe}", file=sys.stderr)
        return 1

    best_icon = select_best_icon(icons)
    print(f"Extracted {len(icons)} icon(s)")
    print(f"Selected: {best_icon.width}x{best_icon.height} @ {best_icon.bpp}bpp")

    if ico_to_png(best_icon.data, output_png):
        print(f"Successfully saved to {output_png}")
        return 0
    else:
        print(f"Error: Failed to convert to PNG", file=sys.stderr)
        print(f"Note: PIL (Pillow) or ImageMagick (magick) is required", file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
