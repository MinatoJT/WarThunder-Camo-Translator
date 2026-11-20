"""Snapshot the saved C4D project and export a copy with new UVW coordinates."""

import json
import os
from pathlib import Path

import c4d


def objects(node, parents=()):
    while node:
        path = (*parents, node.GetName())
        if node.CheckType(c4d.Opolygon):
            yield node, path
        yield from objects(node.GetDown(), path)
        node = node.GetNext()


def get_uvw(obj, path):
    tags = [tag for tag in obj.GetTags() if tag.CheckType(c4d.Tuvw)]
    if len(tags) != 1:
        raise ValueError(f"Expected one UVW tag on {'/'.join(path)}, found {len(tags)}")
    return tags[0]


def snapshot(doc, destination):
    slots = []
    by_name = {}
    info = []
    faces = []
    for object_id, (obj, path) in enumerate(objects(doc.GetFirstObject())):
        uvw = get_uvw(obj, path)
        world = obj.GetMg()
        points = [(p.x, p.y, p.z) for p in (world * p for p in obj.GetAllPoints())]
        info.append(dict(path=path, point_count=len(points), polygon_count=obj.GetPolygonCount()))
        tags = obj.GetTags()
        selections = {tag.GetName(): tag.GetBaseSelect() for tag in reversed(tags)
                      if tag.CheckType(c4d.Tpolygonselection)}
        materials = []
        for tag in reversed(tags):
            if not tag.CheckType(c4d.Ttexture):
                continue
            material = tag.GetMaterial()
            if material is None:
                continue
            selection = tag[c4d.TEXTURETAG_RESTRICTION]
            if not selection or selection in selections:
                materials.append((material.GetName(), selections.get(selection)))

        for polygon, face in enumerate(obj.GetAllPolygons()):
            name = next((name for name, group in materials if group is None or group.IsSelected(polygon)),
                        "Default")
            if name not in by_name:
                by_name[name] = len(slots)
                slots.append(name)
            component = uvw.GetSlow(polygon)
            vertices = (face.a, face.b, face.c, face.d)
            for corners in ((0, 1, 2),) if face.c == face.d else ((0, 1, 2), (0, 2, 3)):
                ids = [vertices[i] for i in corners]
                uv = [component["abcd"[i]] for i in corners]
                faces.append(dict(object=object_id, polygon=polygon, corners=corners, vertices=ids,
                                  slot=by_name[name], positions=[points[i] for i in ids],
                                  original=[(p.x, 1.0 - p.y) for p in uv]))
    with destination.open("w", encoding="utf-8") as output:
        json.dump(dict(slots=slots, objects=info, faces=faces), output, ensure_ascii=False, separators=(",", ":"))


def remove_mesh_keep_children(obj):
    if obj.GetDown() is not None:
        parent = c4d.BaseObject(c4d.Onull)
        parent.SetName(obj.GetName())
        parent.InsertBefore(obj)
        parent.SetMg(obj.GetMg())
        while obj.GetDown() is not None:
            child = obj.GetDown()
            transform = child.GetMg()
            child.Remove()
            child.InsertUnderLast(parent)
            child.SetMg(transform)
    obj.Remove()


def remove_polygons(doc, obj, kept):
    count = obj.GetPolygonCount()
    if len(kept) == count:
        return
    obj.GetPolygonH().DeselectAll()
    selection = obj.GetPolygonS()
    selection.DeselectAll()
    for polygon in range(count):
        if polygon not in kept:
            selection.Select(polygon)
    result = c4d.utils.SendModelingCommand(
        command=c4d.MCOMMAND_DELETE,
        list=[obj],
        mode=c4d.MODELINGCOMMANDMODE_POLYGONSELECTION,
        doc=doc,
    )
    if not result or obj.GetPolygonCount() != len(kept):
        raise RuntimeError(f"Cinema 4D could not remove the scanned polygons from {obj.GetName()}")


def export_fbx(doc, destination, project_path):
    with project_path.open(encoding="utf-8") as source:
        scene = json.load(source)["scene"]
    meshes = list(objects(doc.GetFirstObject()))
    if len(meshes) != len(scene["objects"]):
        raise ValueError("The source scene changed while preparing the project")

    layouts = [{} for _ in meshes]
    for face in scene["faces"]:
        group = layouts[face["object"]].setdefault(face["polygon"], [None] * 4)
        i = face["tile"] - 1001
        for corner, (u, v) in zip(face["corners"], face["paint"]):
            offset = (u + i % 10, 1.0 - v - i // 10)
            if group[corner] is not None and any(abs(a - b) > 1e-7 for a, b in zip(group[corner], offset)):
                raise ValueError("A quad needs to be triangulated in the source before unstacking its overlapping UVs")
            group[corner] = offset

    marker = 0
    for index, (obj, path) in enumerate(meshes):
        expected = scene["objects"][index]
        if (list(path), obj.GetPointCount(), obj.GetPolygonCount()) != (
                expected["path"], expected["point_count"], expected["polygon_count"]):
            raise ValueError(f"The source mesh changed at {'/'.join(path)}")
        layout = layouts[index]
        if not layout:
            remove_mesh_keep_children(obj)
            continue
        uvw = get_uvw(obj, path)
        for polygon, uv in layout.items():
            if uv[3] is None:
                uv[3] = uv[2]
            uvw.SetSlow(polygon, *(c4d.Vector(u, v, 0) for u, v in uv))
        remove_polygons(doc, obj, layout)
        obj.SetName(f"CamoTranslatorMesh{marker}")
        marker += 1
        obj.Message(c4d.MSG_UPDATE)

    plugin = c4d.plugins.FindPlugin(c4d.FORMAT_FBX_EXPORT, c4d.PLUGINTYPE_SCENESAVER)
    options = {}
    if plugin is None or not plugin.Message(c4d.MSG_RETRIEVEPRIVATEDATA, options):
        raise RuntimeError("Cinema 4D FBX exporter is unavailable")
    settings = options["imexporter"]
    for key, value in {
        "FBXEXPORT_ASCII": False,
        "FBXEXPORT_MATERIALS": True,
        "FBXEXPORT_SAVE_NORMALS": True,
        "FBXEXPORT_TEXTURES": False,
        "FBXEXPORT_EMBED_TEXTURES": False,
        "FBXEXPORT_BAKE_MATERIALS": False,
        "FBXEXPORT_SELECTION_ONLY": False,
        "FBXEXPORT_TRIANGULATE": False,
        "FBXEXPORT_ANIMATION": False,
        "FBXEXPORT_LIGHTS": False,
        "FBXEXPORT_CAMERAS": False,
    }.items():
        if hasattr(c4d, key):
            settings[getattr(c4d, key)] = value
    flags = c4d.SAVEDOCUMENTFLAGS_DONTADDTORECENTLIST
    if not c4d.documents.SaveDocument(doc, str(destination), flags, c4d.FORMAT_FBX_EXPORT):
        raise RuntimeError("Cinema 4D could not export the Painter FBX")


def main():
    source = os.environ["CAMO_TRANSLATOR_SCENE"]
    destination = Path(os.environ["CAMO_TRANSLATOR_DESTINATION"])
    flags = c4d.SCENEFILTER_OBJECTS | c4d.SCENEFILTER_MATERIALS
    doc = c4d.documents.LoadDocument(source, flags)
    if doc is None:
        raise RuntimeError(f"Cinema 4D could not open {source}")
    if os.environ["CAMO_TRANSLATOR_OPERATION"] == "snapshot":
        snapshot(doc, destination)
    else:
        export_fbx(doc, destination, Path(os.environ["CAMO_TRANSLATOR_PROJECT"]))
    print("Camo Translator scene operation completed", flush=True)


if __name__ == "__main__":
    main()
